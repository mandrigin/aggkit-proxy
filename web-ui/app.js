// Miden x AggLayer Bridge - Deposit dApp
//
// Sends bridgeAsset() calls to the Agglayer bridge contract,
// mirroring scripts/send-deposit.sh from the browser.

(function () {
  "use strict";

  // --- Configuration ---------------------------------------------------

  // Bridge contract address: injected at build time via env, or fallback.
  const BRIDGE_ADDRESS =
    (window.__MIDEN_BRIDGE_ADDRESS || "").toLowerCase() ||
    "0xc8cbebf950b9df44d987c8619f092bea980ff038";

  // Expected L1 chain ID (injected at build time, default: Kurtosis devnet)
  const L1_CHAIN_ID = window.__MIDEN_L1_CHAIN_ID || 271828;
  const L1_CHAIN_ID_HEX = "0x" + L1_CHAIN_ID.toString(16);

  // L1 RPC URL for MetaMask (injected at build time, port is dynamic per enclave)
  const L1_RPC_URL = window.__MIDEN_L1_RPC_URL || "";

  // Destination network for Miden
  const DEST_NETWORK = 2;

  // bridgeAsset ABI fragment
  const BRIDGE_ABI = [
    "function bridgeAsset(uint32 destinationNetwork, address destinationAddress, uint256 amount, address token, bool forceUpdateGlobalExitRoot, bytes permitData) payable",
  ];

  // --- DOM refs --------------------------------------------------------

  const connectBtn = document.getElementById("connectBtn");
  const depositBtn = document.getElementById("depositBtn");
  const addNetworkBtn = document.getElementById("addNetworkBtn");
  const recipientInput = document.getElementById("recipient");
  const amountInput = document.getElementById("amount");
  const statusEl = document.getElementById("status");
  const balanceHint = document.getElementById("balanceHint");
  const bridgeAddrEl = document.getElementById("bridgeAddr");
  const accountAddrEl = document.getElementById("accountAddr");
  const networkInfoEl = document.getElementById("networkInfo");
  const chainWarning = document.getElementById("chainWarning");

  // --- State -----------------------------------------------------------

  let provider = null;
  let signer = null;
  let userAddress = null;
  let onCorrectChain = false;

  // --- Helpers ---------------------------------------------------------

  function showStatus(msg, type) {
    statusEl.textContent = msg;
    statusEl.className = "status " + type;
  }

  function showStatusHTML(html, type) {
    statusEl.innerHTML = html;
    statusEl.className = "status " + type;
  }

  function hideStatus() {
    statusEl.className = "status hidden";
  }

  function setLoading(btn, loading) {
    if (loading) {
      btn.classList.add("loading");
      btn.disabled = true;
    } else {
      btn.classList.remove("loading");
      btn.disabled = false;
    }
  }

  function truncateAddr(addr) {
    if (!addr || addr.length < 12) return addr;
    return addr.slice(0, 6) + "..." + addr.slice(-4);
  }

  // Convert 15-byte Miden AccountId to 20-byte Ethereum address.
  // Prepends 5 zero bytes (10 hex chars) to the 30-char hex ID.
  function midenToEthAddress(midenAddr) {
    let hex = midenAddr.replace(/^0x/i, "");
    if (hex.length !== 30) {
      throw new Error(
        "Miden address must be 30 hex chars (15 bytes), got " + hex.length
      );
    }
    if (!/^[0-9a-fA-F]+$/.test(hex)) {
      throw new Error("Miden address contains non-hex characters");
    }
    return "0x0000000000" + hex;
  }

  function validateInputs() {
    const addr = recipientInput.value.trim();
    const amt = amountInput.value.trim();

    let addrOk = false;
    if (addr) {
      const hex = addr.replace(/^0x/i, "");
      addrOk = hex.length === 30 && /^[0-9a-fA-F]+$/.test(hex);
    }

    const amtOk = amt && !isNaN(parseFloat(amt)) && parseFloat(amt) > 0;

    depositBtn.disabled = !signer || !addrOk || !amtOk || !onCorrectChain;
  }

  // --- Chain management ------------------------------------------------

  function updateChainWarning(currentChainId) {
    onCorrectChain = BigInt(currentChainId) === BigInt(L1_CHAIN_ID);
    if (onCorrectChain) {
      chainWarning.classList.add("hidden");
      addNetworkBtn.style.display = "none";
    } else {
      chainWarning.classList.remove("hidden");
      addNetworkBtn.style.display = "";
    }
    validateInputs();
  }

  async function switchOrAddChain() {
    if (!window.ethereum) return;

    try {
      // First try switching to the chain (works if already added)
      await window.ethereum.request({
        method: "wallet_switchEthereumChain",
        params: [{ chainId: L1_CHAIN_ID_HEX }],
      });
    } catch (switchError) {
      // 4902 = chain not added to wallet
      if (switchError.code === 4902) {
        if (!L1_RPC_URL) {
          showStatus(
            "Chain not found in MetaMask. Add it manually: Chain ID " +
              L1_CHAIN_ID +
              ", RPC URL: run 'kurtosis port print <enclave> el-1-geth-lighthouse rpc'",
            "error"
          );
          return;
        }
        try {
          await window.ethereum.request({
            method: "wallet_addEthereumChain",
            params: [
              {
                chainId: L1_CHAIN_ID_HEX,
                chainName: "Kurtosis L1 (Miden)",
                nativeCurrency: {
                  name: "Ether",
                  symbol: "ETH",
                  decimals: 18,
                },
                rpcUrls: [L1_RPC_URL],
                blockExplorerUrls: [],
              },
            ],
          });
        } catch (addError) {
          showStatus(
            "Could not add network. Add it manually in MetaMask: Chain ID " +
              L1_CHAIN_ID +
              ", RPC: " + L1_RPC_URL,
            "error"
          );
        }
      } else {
        showStatus("Failed to switch network: " + (switchError.message || switchError), "error");
      }
    }
  }

  // --- Wallet connection ------------------------------------------------

  async function connectWallet() {
    if (!window.ethereum) {
      showStatus(
        "No wallet detected. Install MetaMask or another EIP-1193 wallet.",
        "error"
      );
      return;
    }

    try {
      setLoading(connectBtn, true);
      provider = new ethers.BrowserProvider(window.ethereum);
      const accounts = await provider.send("eth_requestAccounts", []);
      signer = await provider.getSigner();
      userAddress = accounts[0];

      connectBtn.textContent = truncateAddr(userAddress);
      accountAddrEl.textContent = userAddress;
      bridgeAddrEl.textContent = BRIDGE_ADDRESS;

      // Check chain and show network info
      const network = await provider.getNetwork();
      networkInfoEl.textContent =
        (network.name !== "unknown" ? network.name + " / " : "") +
        "Chain " +
        network.chainId.toString();

      updateChainWarning(network.chainId);

      // Show balance
      await updateBalance();

      validateInputs();
      hideStatus();
    } catch (err) {
      showStatus("Connection failed: " + err.message, "error");
    } finally {
      setLoading(connectBtn, false);
      if (userAddress) {
        connectBtn.disabled = false; // keep clickable for re-connect
      }
    }
  }

  async function updateBalance() {
    if (!provider || !userAddress) return;
    try {
      const bal = await provider.getBalance(userAddress);
      const ethBal = ethers.formatEther(bal);
      balanceHint.textContent =
        "Balance: " + parseFloat(ethBal).toFixed(4) + " ETH";
    } catch (_) {
      balanceHint.textContent = "";
    }
  }

  // --- Deposit ----------------------------------------------------------

  async function sendDeposit() {
    hideStatus();

    if (!onCorrectChain) {
      showStatus("Switch to the Kurtosis L1 network before depositing.", "error");
      return;
    }

    const recipientRaw = recipientInput.value.trim();
    const amountRaw = amountInput.value.trim();

    // Validate Miden address
    let destAddress;
    try {
      destAddress = midenToEthAddress(recipientRaw);
    } catch (err) {
      showStatus(err.message, "error");
      return;
    }

    // Parse amount
    let amountWei;
    try {
      amountWei = ethers.parseEther(amountRaw);
    } catch (err) {
      showStatus("Invalid ETH amount.", "error");
      return;
    }

    if (amountWei === 0n) {
      showStatus("Amount must be greater than zero.", "error");
      return;
    }

    try {
      setLoading(depositBtn, true);
      showStatus("Waiting for wallet confirmation...", "info");

      const contract = new ethers.Contract(BRIDGE_ADDRESS, BRIDGE_ABI, signer);

      const tx = await contract.bridgeAsset(
        DEST_NETWORK, // destinationNetwork = 2 (Miden)
        destAddress, // destinationAddress (padded)
        amountWei, // amount
        ethers.ZeroAddress, // token = 0x0 (native ETH)
        true, // forceUpdateGlobalExitRoot
        "0x", // permitData
        { value: amountWei }
      );

      showStatusHTML(
        "Transaction sent: <code>" + tx.hash + "</code><br>Waiting for confirmation...",
        "info"
      );

      const receipt = await tx.wait();

      if (receipt.status === 1) {
        showStatusHTML(
          "Deposit confirmed in block " +
            receipt.blockNumber +
            ".<br>Tx: <code>" +
            tx.hash +
            "</code>",
          "success"
        );
      } else {
        showStatus("Transaction reverted. Tx: " + tx.hash, "error");
      }

      await updateBalance();
    } catch (err) {
      const msg = err.reason || err.message || String(err);
      showStatus("Deposit failed: " + msg, "error");
    } finally {
      setLoading(depositBtn, false);
      validateInputs();
    }
  }

  // --- Event listeners --------------------------------------------------

  connectBtn.addEventListener("click", connectWallet);
  depositBtn.addEventListener("click", sendDeposit);
  addNetworkBtn.addEventListener("click", switchOrAddChain);
  recipientInput.addEventListener("input", validateInputs);
  amountInput.addEventListener("input", validateInputs);

  // Handle account/chain changes
  if (window.ethereum) {
    window.ethereum.on("accountsChanged", function () {
      window.location.reload();
    });
    window.ethereum.on("chainChanged", function (chainId) {
      updateChainWarning(chainId);
      // Re-init provider on chain change
      if (userAddress) {
        provider = new ethers.BrowserProvider(window.ethereum);
        provider.getSigner().then(function (s) { signer = s; });
        updateBalance();
      }
    });
  }

  // Display default bridge address
  bridgeAddrEl.textContent = BRIDGE_ADDRESS;
})();
