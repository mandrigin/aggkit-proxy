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
  const connectText = document.getElementById("connectText"); // New ref for span
  const depositBtn = document.getElementById("depositBtn");
  const addNetworkBtn = document.getElementById("addNetworkBtn");
  const recipientInput = document.getElementById("recipient");
  const amountInput = document.getElementById("amount");
  const statusEl = document.getElementById("status");
  const balanceDisplay = document.getElementById("balanceDisplay"); // Updated ref
  const bridgeAddrEl = document.getElementById("bridgeAddr");
  const accountAddrEl = document.getElementById("accountAddr");
  const networkInfoEl = document.getElementById("networkInfo");
  const chainWarning = document.getElementById("chainWarning");
  const amountError = document.getElementById("amountError"); // New ref

  // --- State -----------------------------------------------------------

  let provider = null;
  let signer = null;
  let userAddress = null;
  let onCorrectChain = false;

  // --- Helpers ---------------------------------------------------------

  function appendStatus(msg, type) {
    statusEl.style.display = 'block'; // Ensure visible using style
    statusEl.classList.remove('hidden');

    // Receipt style: prepend timestamp
    const now = new Date().toLocaleTimeString('en-US', { hour12: false });
    const line = document.createElement('div');
    line.className = 'receipt-item ' + type;
    line.innerHTML = `<span class="receipt-timestamp">[${now}]</span> ${msg}`;

    // Append to bottom
    statusEl.appendChild(line);
    statusEl.scrollTop = statusEl.scrollHeight;
  }

  function clearStatus() {
    statusEl.innerHTML = '';
    statusEl.classList.add('hidden');
    statusEl.style.display = 'none';
  }

  // Preserve legacy showStatus for compatibility if needed, but route to append
  function showStatus(msg, type) {
    appendStatus(msg, type);
  }

  function showStatusHTML(html, type) {
    appendStatus(html, type);
  }

  function hideStatus() {
    // We don't hide status on success/action start immediately to keep the "Receipt" history, 
    // unless we start a fresh flow? For now, let's just not hide it vigorously.
    // statusEl.classList.add("hidden");
  }

  function setLoading(btn, loading) {
    if (loading) {
      btn.classList.add("loading");
      btn.disabled = true;
      const originalText = btn.getAttribute('data-text') || btn.innerText;
      if (!btn.getAttribute('data-text')) btn.setAttribute('data-text', originalText);
      // btn.innerText = "PROCESSING..."; // Don't change text for trigger-btn logic widely
    } else {
      btn.classList.remove("loading");
      btn.disabled = false;
      // if(btn.getAttribute('data-text')) btn.innerText = btn.getAttribute('data-text');
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
    recipientInput.style.color = addrOk ? 'var(--c-ink)' : (addr.length > 0 ? 'var(--c-alert)' : 'var(--c-ink)');

    const amtOk = amt && !isNaN(parseFloat(amt)) && parseFloat(amt) > 0;

    // Check balance if connected
    if (amtOk && provider && userAddress) {
      // Ideally we check against actual balance here, but we need the BigInt balance.
      // For now, strict validation on format.
    }

    depositBtn.disabled = !signer || !addrOk || !amtOk || !onCorrectChain;
  }

  // --- Chain management ------------------------------------------------

  function updateChainWarning(currentChainId) {
    onCorrectChain = BigInt(currentChainId) === BigInt(L1_CHAIN_ID);
    if (onCorrectChain) {
      chainWarning.classList.add("hidden");
      chainWarning.style.display = "none";
      addNetworkBtn.style.display = "none";
    } else {
      chainWarning.classList.remove("hidden");
      chainWarning.style.display = "block";
      addNetworkBtn.style.display = "";
    }
    validateInputs();
  }

  async function switchOrAddChain() {
    if (!window.ethereum) return;

    try {
      await window.ethereum.request({
        method: "wallet_switchEthereumChain",
        params: [{ chainId: L1_CHAIN_ID_HEX }],
      });
    } catch (switchError) {
      if (switchError.code === 4902) {
        if (!L1_RPC_URL) {
          showStatus("Chain not found in MetaMask. Add manually: " + L1_CHAIN_ID, "error");
          return;
        }
        try {
          await window.ethereum.request({
            method: "wallet_addEthereumChain",
            params: [
              {
                chainId: L1_CHAIN_ID_HEX,
                chainName: "Kurtosis L1 (Miden)",
                nativeCurrency: { name: "Ether", symbol: "ETH", decimals: 18 },
                rpcUrls: [L1_RPC_URL],
                blockExplorerUrls: [],
              },
            ],
          });
        } catch (addError) {
          showStatus("Could not add network: " + addError.message, "error");
        }
      } else {
        showStatus("Failed to switch: " + switchError.message, "error");
      }
    }
  }

  // --- Wallet connection ------------------------------------------------

  async function connectWallet() {
    clearStatus(); // Start fresh receipt on connect attempt
    if (!window.ethereum) {
      showStatus("No wallet detected. Install MetaMask.", "error");
      return;
    }

    try {
      setLoading(connectBtn, true);
      provider = new ethers.BrowserProvider(window.ethereum);
      const accounts = await provider.send("eth_requestAccounts", []);
      signer = await provider.getSigner();
      userAddress = accounts[0];

      // Update Port UI
      connectBtn.classList.add('connected');
      if (connectText) connectText.textContent = truncateAddr(userAddress);

      accountAddrEl.textContent = userAddress;
      bridgeAddrEl.textContent = BRIDGE_ADDRESS;

      const network = await provider.getNetwork();
      networkInfoEl.textContent = `${network.chainId} (${network.name})`;

      updateChainWarning(network.chainId);
      await updateBalance();

      validateInputs();
      showStatus("Wallet connected successfully.", "info");
    } catch (err) {
      showStatus("Connection failed: " + err.message, "error");
    } finally {
      setLoading(connectBtn, false);
      if (userAddress) connectBtn.disabled = false;
    }
  }

  async function updateBalance() {
    if (!provider || !userAddress) return;
    try {
      const bal = await provider.getBalance(userAddress);
      const ethBal = ethers.formatEther(bal);
      balanceDisplay.textContent = parseFloat(ethBal).toFixed(4);

      // Simple error check for amount vs balance could go here
    } catch (_) {
      balanceDisplay.textContent = "--";
    }
  }

  // --- Deposit ----------------------------------------------------------

  async function sendDeposit() {
    // Don't clear status, we want to see the sequence
    if (!onCorrectChain) {
      showStatus("WRONG NETWORK. Switch to L1.", "error");
      return;
    }

    const recipientRaw = recipientInput.value.trim();
    const amountRaw = amountInput.value.trim();

    let destAddress;
    try {
      destAddress = midenToEthAddress(recipientRaw);
    } catch (err) {
      showStatus("INVALID ADDR: " + err.message, "error");
      return;
    }

    let amountWei;
    try {
      amountWei = ethers.parseEther(amountRaw);
    } catch (err) {
      showStatus("INVALID AMOUNT", "error");
      return;
    }

    try {
      setLoading(depositBtn, true);
      showStatus("INITIATING BRIDGE TRANSACTION...", "info");

      const contract = new ethers.Contract(BRIDGE_ADDRESS, BRIDGE_ABI, signer);

      const tx = await contract.bridgeAsset(
        DEST_NETWORK,
        destAddress,
        amountWei,
        ethers.ZeroAddress,
        true,
        "0x",
        { value: amountWei }
      );

      showStatusHTML(`TX SIGNED: <code>${tx.hash}</code> (WAITING CONFIRMATION)`, "info");

      const receipt = await tx.wait();

      if (receipt.status === 1) {
        showStatusHTML(`DEPOSIT CONFIRMED (BLOCK ${receipt.blockNumber})`, "success");
      } else {
        showStatus(`TX REVERTED: ${tx.hash}`, "error");
      }

      await updateBalance();
      // Clear inputs on success? Maybe not for UX, let user decide.
      amountInput.value = "";
    } catch (err) {
      const msg = err.reason || err.message || String(err);
      showStatus("DEPOSIT FAILED: " + msg, "error");
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

  if (window.ethereum) {
    window.ethereum.on("accountsChanged", () => window.location.reload());
    window.ethereum.on("chainChanged", (chainId) => {
      updateChainWarning(chainId);
      if (userAddress) {
        provider = new ethers.BrowserProvider(window.ethereum);
        provider.getSigner().then((s) => { signer = s; });
        updateBalance();
      }
    });
  }

  bridgeAddrEl.textContent = BRIDGE_ADDRESS;

  // --- Scramble & Animation Effects ------------------------------------

  function scrambleText(element) {
    const originalText = element.getAttribute("data-text") || element.innerText;
    const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%^&*()_+-=[]{}|;:,.<>?/";
    let iterations = 0;

    // Preserve line breaks if any (simple approach) or just use raw text
    // For "DEPOSIT<br>ETH", innerText is "DEPOSIT\nETH", which is fine to scramble line by line
    // But for simplicity, let's just scramble the visible text content linearly.

    // Actually, handling <br> is tricky with simple text replacement.
    // Let's target specific elements or assume single line for now, 
    // OR just scramble the textContent and ignore HTML structure temporarily (might break layout).
    // Better approach: Split by lines if existing.

    const lines = originalText.split('<br>'); // Simple check if data-text has it
    // If complex, fallback to simple scrambling

    clearInterval(element.interval);

    element.interval = setInterval(() => {
      element.innerText = originalText
        .split("")
        .map((letter, index) => {
          if (index < iterations) {
            return originalText[index];
          }
          if (letter === " " || letter === "\n" || letter === "<" || letter === ">") return letter;
          return chars[Math.floor(Math.random() * chars.length)];
        })
        .join("");

      if (iterations >= originalText.length) {
        clearInterval(element.interval);
        // Restore HTML if needed (e.g. <br>)
        element.innerHTML = originalText;
      }

      iterations += 1 / 3;
    }, 30);
  }

  // Trigger on H1 load
  const heroTitle = document.getElementById("heroTitle");
  if (heroTitle) {
    // Initial scramble
    setTimeout(() => scrambleText(heroTitle), 500);

    // Re-scramble on hover? Maybe too aggressive. Let's keep it load-only or specific trigger.
    heroTitle.addEventListener("mouseover", () => {
      // scrambleText(heroTitle); // Uncomment for chaos
    });
  }

  // --- Mouse Spotlight -------------------------------------------------
  const gridLines = document.querySelector('.grid-lines');
  if (gridLines) {
    document.addEventListener('mousemove', (e) => {
      gridLines.style.setProperty('--mouse-x', e.clientX + 'px');
      gridLines.style.setProperty('--mouse-y', e.clientY + 'px');
    });
  }

})();
