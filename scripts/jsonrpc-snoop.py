#!/usr/bin/env python3
"""JSON-RPC snooping reverse proxy with event decoding and block number tracking."""
import sys, json, time, threading
from http.server import HTTPServer, BaseHTTPRequestHandler
from urllib.request import Request, urlopen
from urllib.error import URLError

UPSTREAM = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:62178"
LISTEN_PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 9999
LOG_FILE = sys.argv[3] if len(sys.argv) > 3 else "/tmp/jsonrpc-snoop.log"

KNOWN_EVENTS = {
    # keccak256("InsertGlobalExitRoot(bytes32,bytes32)") — sovereign chain GER injection
    "0x65d3bf36615f1f02a134d12dfa9ea6b1d4a52386e825973cd27ddb70895c2319":
        ("InsertGlobalExitRoot", ["bytes32 newGER", "bytes32 lastBlockHash"]),
    # keccak256("ClaimEvent(uint256,uint32,address,address,uint256)") — from log_synthesis.rs
    "0x1df3f2a973a00d6635911755c260704e95e8a5876997546798770f76396fda4d":
        ("ClaimEvent", ["uint256 globalIndex", "uint32 originNet", "address originAddr", "address destAddr", "uint256 amount"]),
    # keccak256("BridgeEvent(uint8,uint32,address,uint32,address,uint256,bytes,uint32)") — from log_synthesis.rs
    "0x501781209a1f8899323b96b4ef08b168df93e0a90c673d1e4cce39366cb62f9b":
        ("BridgeEvent", ["uint8 leafType", "uint32 originNet", "address originAddr", "uint32 destNet", "address destAddr", "uint256 amount"]),
}

log_lock = threading.Lock()
log_fh = open(LOG_FILE, "a", buffering=1)

def log(msg):
    ts = time.strftime("%H:%M:%S", time.localtime()) + f".{int(time.time()*1000)%1000:03d}"
    line = f"{ts} {msg}"
    with log_lock:
        log_fh.write(line + "\n")
        print(line, flush=True)

def hex2int(h):
    try: return int(h, 16)
    except: return h

def decode_log_entry(entry):
    topics = entry.get("topics", [])
    block = entry.get("blockNumber", "?")
    block_dec = hex2int(block) if isinstance(block, str) else block
    tx = entry.get("transactionHash", "?")
    topic0 = (topics[0].lower() if topics else "")
    ev = KNOWN_EVENTS.get(topic0)
    name = ev[0] if ev else f"unknown({topic0[:18]}...)"
    indexed = []
    for t in topics[1:]:
        indexed.append(t[:18] + "...")
    return f"    event block={block_dec} {name}({', '.join(indexed)}) tx={tx[:18]}..."

class SnoopHandler(BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(length)
        try: req = json.loads(body)
        except: req = {}

        method = req.get("method", "?")
        params = req.get("params", [])
        req_id = req.get("id", "?")

        # Format request based on method
        if method == "eth_getLogs" and params:
            p = params[0] if isinstance(params, list) and params else {}
            fb = p.get("fromBlock", "?")
            tb = p.get("toBlock", "?")
            fb_dec = hex2int(fb)
            tb_dec = hex2int(tb)
            req_str = f">> eth_getLogs(from={fb_dec}, to={tb_dec})"
        elif method == "eth_getBlockByNumber" and params:
            bn = params[0] if isinstance(params, list) else "?"
            bn_dec = hex2int(bn) if bn not in ("latest","pending","finalized","safe","earliest") else bn
            req_str = f">> eth_getBlockByNumber({bn_dec})"
        elif method == "eth_blockNumber":
            req_str = f">> eth_blockNumber()"
        elif method == "eth_sendRawTransaction":
            req_str = f">> eth_sendRawTransaction(...)"
        elif method == "eth_getTransactionReceipt":
            tx = params[0][:18] + "..." if params else "?"
            req_str = f">> eth_getTransactionReceipt({tx})"
        else:
            ps = json.dumps(params, separators=(",",":"))
            if len(ps) > 60: ps = ps[:60] + "..."
            req_str = f">> {method}({ps})"

        log(req_str)

        try:
            upstream_req = Request(UPSTREAM, data=body, headers={"Content-Type": "application/json"}, method="POST")
            with urlopen(upstream_req, timeout=120) as resp:
                resp_body = resp.read()
                status = resp.status
        except Exception as e:
            log(f"<< ERROR: {e}")
            self.send_error(502, str(e))
            return

        try: resp_json = json.loads(resp_body)
        except: resp_json = {}

        result = resp_json.get("result")
        error = resp_json.get("error")

        # Format response based on method
        if error:
            msg = error.get("message", "")[:80]
            log(f"<< {method} => ERROR: {msg}")
        elif method == "eth_blockNumber":
            log(f"<< eth_blockNumber => {hex2int(result)} ({result})")
        elif method == "eth_getBlockByNumber":
            num = result.get("number", "?") if isinstance(result, dict) else "?"
            log(f"<< eth_getBlockByNumber => block {hex2int(num)}")
        elif method == "eth_getLogs":
            items = result if isinstance(result, list) else []
            log(f"<< eth_getLogs => [{len(items)} items]")
            for entry in items:
                log(decode_log_entry(entry))
        elif method == "eth_sendRawTransaction":
            if isinstance(result, str):
                log(f"<< eth_sendRawTransaction => {result[:22]}...")
            else:
                log(f"<< eth_sendRawTransaction => {result}")
        elif method == "eth_getTransactionReceipt":
            if isinstance(result, dict):
                bn = result.get("blockNumber", "?")
                st = result.get("status", "?")
                log(f"<< eth_getTransactionReceipt => block={hex2int(bn)} status={st}")
            else:
                log(f"<< eth_getTransactionReceipt => {result}")
        else:
            r = str(result)[:60] if result else "null"
            log(f"<< {method} => {r}")

        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(resp_body)))
        self.end_headers()
        self.wfile.write(resp_body)

    def log_message(self, *a): pass

log(f"Snoop proxy: :{LISTEN_PORT} -> {UPSTREAM}")
server = HTTPServer(("0.0.0.0", LISTEN_PORT), SnoopHandler)
server.serve_forever()
