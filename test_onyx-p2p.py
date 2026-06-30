import subprocess
import time
import threading
import sys
import platform

def get_bin_path():
    ext = ".exe" if platform.system() == "Windows" else ""
    return f"target/release/onyx-p2p{ext}"

def reader_thread(p, output_list, name):
    try:
        for line in p.stdout:
            output_list.append(line)
            with open("test_output.log", "a", encoding="utf-8") as f:
                f.write(f"[{name}] {line}")
    except Exception:
        pass

def run_host():
    print("Starting Host...")
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
        
        time.sleep(5)
        
        p.terminate()
        p.wait(timeout=2)
        
        stdout = "".join(out)
        if "Hello from Connect!" in stdout:
            print("HOST TEST PASSED")
        else:
            print("HOST TEST FAILED")
            
    except Exception as e:
        print(f"Host error: {e}")

def run_connect():
    time.sleep(1) # Wait for Host to bind
    print("Starting Connect...")
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
        
        time.sleep(3) # Wait for handshake
        
        p.stdin.write("Hello from Connect!\n")
        p.stdin.flush()
        
        time.sleep(5)
        
        p.terminate()
        p.wait(timeout=2)
        
        stdout = "".join(out)
        if "Hello from Host!" in stdout:
            print("CONNECT TEST PASSED")
        else:
            print("CONNECT TEST FAILED")
            
    except Exception as e:
        print(f"Connect error: {e}")

if __name__ == "__main__":
    t1 = threading.Thread(target=run_host)
    t2 = threading.Thread(target=run_connect)
    t1.start()
    t2.start()
    t1.join()
    t2.join()
    print("Test script finished.")
