import subprocess
import time
import threading
import os
import platform

def get_bin_path():
    ext = ".exe" if platform.system() == "Windows" else ""
    return f"target/release/onyx-p2p{ext}"

def reader_thread(p, output_list, name):
    try:
        for line in p.stdout:
            output_list.append(line)
            with open("test_results.log", "a", encoding="utf-8") as f:
                f.write(f"[{name}] {line}")
    except Exception:
        pass

with open("dummy.txt", "w", encoding="utf-8") as f:
    f.write("This is a highly secret file transfer test payload." * 100)

with open("test_results.log", "w", encoding="utf-8") as f:
    f.write("Starting test...\n")

def log(msg):
    with open("test_results.log", "a", encoding="utf-8") as f:
        f.write(msg + "\n")

def run_host():
    log("[HOST] Starting Host...")
    try:
        p = subprocess.Popen([get_bin_path()], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, encoding="utf-8", errors="replace")
        
        out = []
        t = threading.Thread(target=reader_thread, args=(p, out, "HOST"))
        t.daemon = True
        t.start()
        
        # 1. Menu
        p.stdin.write("1\n")
        # 2. Name
        p.stdin.write("HostUser\n")
        # 3. Password
        p.stdin.write("testpass\n")
        p.stdin.flush()
        
        time.sleep(3) # Wait for connect to join
        
        p.stdin.write("Hello from Host!\n")
        p.stdin.flush()
        
        time.sleep(2)
        
        p.stdin.write("/send dummy.txt\n")
        p.stdin.flush()
        
        time.sleep(8)
        
        p.terminate()
        p.wait(timeout=2)
        
        stdout = "".join(out)
        success_chat = "Hello from Connect!" in stdout
        success_file = "File transmission complete" in stdout or "File sent successfully" in stdout
        
        if success_chat and success_file:
            log("HOST TEST FULLY PASSED")
            print("HOST TEST FULLY PASSED")
        else:
            log(f"HOST TEST FAILED: chat={success_chat}, file={success_file}")
            print(f"HOST TEST FAILED: chat={success_chat}, file={success_file}")
            
    except Exception as e:
        log(f"Host error: {e}")
        print(f"Host error: {e}")

def run_connect():
    time.sleep(1) # Wait for Host to bind
    log("[CONNECT] Starting Connect...")
    try:
        p = subprocess.Popen([get_bin_path()], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, encoding="utf-8", errors="replace")
        
        out = []
        t = threading.Thread(target=reader_thread, args=(p, out, "CONNECT"))
        t.daemon = True
        t.start()
        
        # 1. Menu
        p.stdin.write("2\n")
        # 2. IP
        p.stdin.write("127.0.0.1\n")
        # 3. Name
        p.stdin.write("ConnectUser\n")
        # 4. Password
        p.stdin.write("testpass\n")
        p.stdin.flush()
        
        time.sleep(3) # Wait for handshake and host message
        
        p.stdin.write("Hello from Connect!\n")
        p.stdin.flush()
        
        # We need to accept the file transfer in the middle!
        time.sleep(3)
        p.stdin.write("/accept\n")
        p.stdin.flush()
        
        time.sleep(5)
        
        p.terminate()
        p.wait(timeout=2)
        
        stdout = "".join(out)
        success_chat = "Hello from Host!" in stdout
        success_file = False
        
        if os.path.exists("downloaded_dummy.txt"):
            with open("downloaded_dummy.txt", "r", encoding="utf-8") as f:
                content = f.read()
                if len(content) > 100:
                    success_file = True
                    log("[CONNECT] Downloaded file integrity verified.")
                    print("[CONNECT] Downloaded file integrity verified.")

        if success_chat and success_file:
            log("CONNECT TEST FULLY PASSED")
            print("CONNECT TEST FULLY PASSED")
        else:
            log(f"CONNECT TEST FAILED: chat={success_chat}, file={success_file}")
            print(f"CONNECT TEST FAILED: chat={success_chat}, file={success_file}")
            
    except Exception as e:
        log(f"Connect error: {e}")
        print(f"Connect error: {e}")

if __name__ == "__main__":
    t1 = threading.Thread(target=run_host)
    t2 = threading.Thread(target=run_connect)
    t1.start()
    t2.start()
    t1.join()
    t2.join()
    log("Test script finished.")
    print("Test script finished.")
