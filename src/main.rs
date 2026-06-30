use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::Aes256Gcm;
use chacha20poly1305::ChaCha20Poly1305;
use ml_kem::{MlKem768, EncapsulationKey768, KeyExport};
use kem::{Kem, Encapsulate, TryDecapsulate};
use std::ops::Deref;
use colored::*;
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand::{rngs::OsRng, Rng, RngCore};
use rustyline::error::ReadlineError;
use rustyline::{Editor, ExternalPrinter, Cmd, KeyEvent, KeyCode, Modifiers};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::completion::Completer;
use rustyline::Helper;
use std::borrow::Cow;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::{Zeroize, Zeroizing};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

struct MyHelper;

impl Completer for MyHelper {
    type Candidate = String;
}
impl Hinter for MyHelper {
    type Hint = String;
}
impl Validator for MyHelper {}
impl Highlighter for MyHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _default: bool,
    ) -> Cow<'b, str> {
        // Apply green and bold to the prompt right before rendering
        Cow::Owned(format!("\x1B[1;32m{}\x1B[0m", prompt))
    }
}
impl Helper for MyHelper {}

type HmacSha256 = Hmac<Sha256>;

struct SessionKeys {
    pub send_text_key: Zeroizing<[u8; 32]>,
    pub recv_text_key: Zeroizing<[u8; 32]>,
    pub send_file_key: Zeroizing<[u8; 32]>,
    pub recv_file_key: Zeroizing<[u8; 32]>,
    pub use_aes: bool,
}

enum CipherEngine {
    Aes(Aes256Gcm),
    ChaCha(ChaCha20Poly1305),
}

impl CipherEngine {
    fn new(key: &[u8; 32], use_aes: bool) -> Self {
        if use_aes {
            CipherEngine::Aes(Aes256Gcm::new(key.into()))
        } else {
            CipherEngine::ChaCha(ChaCha20Poly1305::new(key.into()))
        }
    }

    fn encrypt(&self, nonce: &[u8; 12], text: &[u8]) -> Option<Vec<u8>> {
        match self {
            CipherEngine::Aes(c) => c.encrypt(nonce.into(), Payload { msg: text, aad: &[] }).ok(),
            CipherEngine::ChaCha(c) => c.encrypt(nonce.into(), Payload { msg: text, aad: &[] }).ok(),
        }
    }

    fn decrypt(&self, nonce: &[u8; 12], ciphertext: &[u8]) -> Option<Vec<u8>> {
        match self {
            CipherEngine::Aes(c) => c.decrypt(nonce.into(), Payload { msg: ciphertext, aad: &[] }).ok(),
            CipherEngine::ChaCha(c) => c.decrypt(nonce.into(), Payload { msg: ciphertext, aad: &[] }).ok(),
        }
    }
}

static GLOBAL_SEND_COUNTER: AtomicU64 = AtomicU64::new(1);
static GLOBAL_RECV_COUNTER: AtomicU64 = AtomicU64::new(0);
static IS_TRANSFERRING: AtomicBool = AtomicBool::new(false);
static PENDING_FILE_REQ: AtomicBool = AtomicBool::new(false);

fn clear_screen() {
    let _ = if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/c", "cls"]).status()
    } else {
        Command::new("clear").status()
    };
}

#[cfg(target_os = "windows")]
fn enable_anti_screenshot() {
    unsafe {
        use windows_sys::Win32::System::Console::GetConsoleWindow;
        use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE};
        let hwnd = GetConsoleWindow();
        if hwnd != 0 as _ {
            SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn enable_anti_screenshot() {}

fn print_banner() {
    clear_screen();
    let banner = r#"
 ▒█████      ███▄    █    ▓██   ██▓   ▒██   ██▒
▒██▒  ██▒    ██ ▀█   █     ▒██  ██▒   ▒▒ █ █ ▒░
▒██░  ██▒   ▓██  ▀█ ██▒     ▒██ ██░   ░░  █   ░
▒██   ██░   ▓██▒  ▐▌██▒     ░ ▐██▓░    ░ █ █ ▒ 
░ ████▓▒░   ▒██░   ▓██░     ░ ██▒▓░   ▒██▒ ▒██▒
░ ▒░▒░▒░    ░ ▒░   ▒ ▒       ██▒▒▒    ▒▒ ░ ░▓ ░
  ░ ▒ ▒░    ░ ░░   ░ ▒░    ▓██ ░▒░    ░░   ░▒ ░
░ ░ ░ ▒        ░   ░ ░     ▒ ▒ ░░      ░    ░  
    ░ ░              ░     ░ ░         ░    ░  
                           ░ ░                 
"#;
    let banner_clean = banner.replace("\r", "");
    let lines: Vec<&str> = banner_clean.trim_matches('\n').split('\n').collect();
    let (r_start, g_start, b_start) = (0.0, 255.0, 102.0); // Neon green
    let (r_end, g_end, b_end) = (0.0, 150.0, 255.0);   // Deep blue

    for (i, line) in lines.iter().enumerate() {
        // Fix math: divide by len - 1 so the last line is fully 1.0 (violet)
        let ratio = i as f64 / (lines.len().saturating_sub(1)) as f64;
        let r = (r_start + ratio * (r_end - r_start)) as u8;
        let g = (g_start + ratio * (g_end - g_start)) as u8;
        let b = (b_start + ratio * (b_end - b_start)) as u8;

        // Force raw 24-bit truecolor ANSI escape sequences to bypass any colored crate downgrades
        println!("\x1B[38;2;{};{};{}m\x1B[1m{}\x1B[0m", r, g, b, line);
    }
    println!();
}

fn ratchet_key(chain_key: &mut [u8; 32]) -> [u8; 32] {
    let hkdf = Hkdf::<Sha256>::new(None, chain_key.as_ref());
    let mut okm = [0u8; 64];
    hkdf.expand(&[], &mut okm).unwrap();
    let mut message_key = [0u8; 32];
    message_key.copy_from_slice(&okm[..32]);
    chain_key.copy_from_slice(&okm[32..]);
    message_key
}

fn encrypt_and_send_message(
    ratchet_mutex: &Arc<Mutex<[u8; 32]>>, 
    text: &str, 
    use_aes: bool,
    write_mutex: &Arc<Mutex<TcpStream>>
) {
    if let Ok(conn_lock) = write_mutex.lock() {
        let message_key = {
            let mut chain_key = ratchet_mutex.lock().unwrap();
            ratchet_key(&mut *chain_key)
        };
        
        let cipher = CipherEngine::new(&message_key, use_aes);
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes[..4]);

        let counter = GLOBAL_SEND_COUNTER.fetch_add(1, Ordering::SeqCst);
        nonce_bytes[4..].copy_from_slice(&counter.to_be_bytes());

        if let Some(ciphertext) = cipher.encrypt(&nonce_bytes, text.as_bytes()) {
            let mut payload = nonce_bytes.to_vec();
            payload.extend(ciphertext);
            let _ = tls_write_record(&*conn_lock, &payload);
        }
    }
}

fn decrypt_message(ratchet_mutex: &Arc<Mutex<[u8; 32]>>, payload: &[u8], use_aes: bool) -> Option<String> {
    if payload.len() < 12 {
        return None;
    }
    
    // Create a snapshot of the current chain key to allow rollback on failure
    let mut chain_key_lock = ratchet_mutex.lock().unwrap();
    let original_chain_key = *chain_key_lock;

    let message_key = ratchet_key(&mut *chain_key_lock);

    let mut nonce_bytes = [0u8; 12];
    nonce_bytes.copy_from_slice(&payload[..12]);
    let ciphertext = &payload[12..];

    let mut counter_bytes = [0u8; 8];
    counter_bytes.copy_from_slice(&nonce_bytes[4..]);
    let msg_counter = u64::from_be_bytes(counter_bytes);

    let current_recv = GLOBAL_RECV_COUNTER.load(Ordering::SeqCst);
    if msg_counter <= current_recv {
        *chain_key_lock = original_chain_key; // Rollback
        return None;
    }

    let cipher = CipherEngine::new(&message_key, use_aes);
    match cipher.decrypt(&nonce_bytes, ciphertext) {
        Some(plaintext) => {
            GLOBAL_RECV_COUNTER.store(msg_counter, Ordering::SeqCst);
            String::from_utf8(plaintext).ok()
        }
        None => {
            *chain_key_lock = original_chain_key; // Rollback
            None
        }
    }
}

fn tls_write_record(mut conn: &TcpStream, data: &[u8]) -> std::io::Result<()> {
    let len = data.len() as u16;
    let header = [0x17, 0x03, 0x03, (len >> 8) as u8, (len & 0xFF) as u8];
    conn.write_all(&header)?;
    conn.write_all(data)?;
    Ok(())
}

fn tls_read_record(mut conn: &TcpStream) -> std::io::Result<Vec<u8>> {
    let mut header = [0u8; 5];
    conn.read_exact(&mut header)?;
    if header[0] != 0x17 || header[1] != 0x03 || header[2] != 0x03 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid TLS Header"));
    }
    let len = ((header[3] as u16) << 8) | (header[4] as u16);
    let mut buf = vec![0u8; len as usize];
    conn.read_exact(&mut buf)?;
    Ok(buf)
}

fn generate_spa_token(password_hash: &[u8; 32]) -> [u8; 32] {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(password_hash).unwrap();
    mac.update(b"ONYX_SPA_AUTH_V1");
    let result = mac.finalize().into_bytes();
    let mut token = [0u8; 32];
    token.copy_from_slice(&result);
    token
}

fn send_fake_client_hello(mut conn: &TcpStream) {
    let mut random = [0u8; 32];
    OsRng.fill_bytes(&mut random);
    let mut hello = vec![
        0x16, 0x03, 0x01, 0x00, 0xc8,
        0x01, 0x00, 0x00, 0xc4,
        0x03, 0x03,
    ];
    hello.extend(&random);
    hello.extend(vec![0; 160]);
    let _ = conn.write_all(&hello);
}

fn read_fake_client_hello(mut conn: &TcpStream) -> Result<(), String> {
    let mut buf = [0u8; 203];
    conn.read_exact(&mut buf).map_err(|_| "Failed to read Fake ClientHello".to_string())?;
    Ok(())
}

fn send_fake_server_hello(mut conn: &TcpStream) {
    let mut random = [0u8; 32];
    OsRng.fill_bytes(&mut random);
    let mut hello = vec![
        0x16, 0x03, 0x03, 0x00, 0x5a,
        0x02, 0x00, 0x00, 0x56,
        0x03, 0x03,
    ];
    hello.extend(&random);
    hello.extend(vec![0; 50]);
    let _ = conn.write_all(&hello);
}

fn read_fake_server_hello(mut conn: &TcpStream) -> Result<(), String> {
    let mut buf = [0u8; 93];
    conn.read_exact(&mut buf).map_err(|_| "Failed to read Fake ServerHello".to_string())?;
    Ok(())
}

fn secure_handshake(mut conn: &TcpStream, mut password_hash: Zeroizing<[u8; 32]>, is_host: bool) -> Result<SessionKeys, String> {
    // ---- PHASE 0: Single Packet Authorization (Silent Drop) ----
    let expected_token = generate_spa_token(&password_hash);
    if is_host {
        let mut client_token = [0u8; 32];
        let _ = conn.set_read_timeout(Some(std::time::Duration::from_secs(3)));
        if conn.read_exact(&mut client_token).is_err() || client_token != expected_token {
            return Err("SPA Failed: Unauthorized probe detected. Connection dropped silently.".to_string());
        }
        let _ = conn.set_read_timeout(None);
    } else {
        conn.write_all(&expected_token).map_err(|_| "Failed to send SPA token")?;
    }

    // ---- PHASE 1: TLS Masquerading ----
    if is_host {
        read_fake_client_hello(&conn)?;
        send_fake_server_hello(&conn);
    } else {
        send_fake_client_hello(&conn);
        read_fake_server_hello(&conn)?;
    }

    // ---- ROUND 1: Exchange Public Keys (X25519 + ML-KEM) ----
    
    let private_key = StaticSecret::random_from_rng(OsRng);
    let public_key = PublicKey::from(&private_key);

    let (dk, ek) = MlKem768::generate_keypair();
    let ek_bytes = ek.to_bytes();

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    let local_aes = std::is_x86_feature_detected!("aes");
    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    let local_aes = false;

    let mut pt1 = Vec::new();
    pt1.extend_from_slice(public_key.as_bytes());
    pt1.extend_from_slice(ek_bytes.as_ref());
    pt1.push(if local_aes { 1 } else { 0 });

    let chacha = ChaCha20Poly1305::new(password_hash.as_ref().into());
    
    let mut nonce1 = [0u8; 12];
    OsRng.fill_bytes(&mut nonce1);
    
    let ct1 = chacha.encrypt(&nonce1.into(), Payload { msg: &pt1, aad: &[] })
        .map_err(|_| "Encryption failed in Round 1")?;

    let mut payload1 = Vec::new();
    payload1.extend_from_slice(&nonce1);
    payload1.extend_from_slice(&ct1);

    tls_write_record(&conn, &payload1).map_err(|e| e.to_string())?;

    // Read Peer Round 1 (12 bytes nonce + 1217 bytes pt + 16 bytes tag = 1245 bytes)
    let peer_payload1 = tls_read_record(&conn).map_err(|_| "Failed to read Round 1 payload".to_string())?;
    if peer_payload1.len() != 1245 { return Err("Invalid Round 1 size".to_string()); }

    let mut peer_nonce1 = [0u8; 12];
    peer_nonce1.copy_from_slice(&peer_payload1[..12]);
    let peer_ct1 = &peer_payload1[12..];

    let peer_pt1 = chacha.decrypt(&peer_nonce1.into(), Payload { msg: peer_ct1, aad: &[] })
        .map_err(|_| "Cryptographic signature mismatch in Round 1 (incorrect password or MitM attack)".to_string())?;

    if peer_pt1.len() != 1217 {
        return Err("Invalid Round 1 payload size".into());
    }

    let mut peer_pub_bytes = [0u8; 32];
    peer_pub_bytes.copy_from_slice(&peer_pt1[..32]);
    let peer_public_key = PublicKey::from(peer_pub_bytes);
    
    let peer_ek_array: &[u8; 1184] = peer_pt1[32..1216].try_into().map_err(|_| "Invalid Kyber key len")?;
    let peer_ek_decoded = EncapsulationKey768::new(peer_ek_array.into()).map_err(|_| "Invalid Kyber key")?;
    let peer_aes_byte = peer_pt1[1216];

    // ---- ROUND 2: KEM Encapsulation Exchange ----

    let (ct_send, ss_send) = peer_ek_decoded.encapsulate();
    let ct_send_bytes: &[u8] = ct_send.deref();

    let mut nonce2 = [0u8; 12];
    OsRng.fill_bytes(&mut nonce2);

    let ct2 = chacha.encrypt(&nonce2.into(), Payload { msg: ct_send_bytes, aad: &[] })
        .map_err(|_| "Encryption failed in Round 2")?;

    let mut payload2 = Vec::new();
    payload2.extend_from_slice(&nonce2);
    payload2.extend_from_slice(&ct2);

    tls_write_record(&conn, &payload2).map_err(|e| e.to_string())?;

    // Read Peer Round 2 (12 bytes nonce + 1088 bytes pt + 16 bytes tag = 1116 bytes)
    let peer_payload2 = tls_read_record(&conn).map_err(|_| "Failed to read Round 2 payload".to_string())?;
    if peer_payload2.len() != 1116 { return Err("Invalid Round 2 size".to_string()); }

    let mut peer_nonce2 = [0u8; 12];
    peer_nonce2.copy_from_slice(&peer_payload2[..12]);
    let peer_ct2 = &peer_payload2[12..];

    let peer_pt2 = chacha.decrypt(&peer_nonce2.into(), Payload { msg: peer_ct2, aad: &[] })
        .map_err(|_| "Cryptographic signature mismatch in Round 2".to_string())?;

    let peer_ct_decoded = peer_pt2.as_slice().try_into().map_err(|_| "Invalid Kyber ciphertext len")?;
    let ss_recv = dk.try_decapsulate(&peer_ct_decoded).map_err(|_| "Kyber decapsulation failed")?;

    // ---- MASTER SECRET DERIVATION ----
    
    let x25519_ss = private_key.diffie_hellman(&peer_public_key);

    let mut master_entropy = Vec::new();
    master_entropy.extend_from_slice(x25519_ss.as_bytes());
    if is_host {
        master_entropy.extend_from_slice(ss_send.as_slice());
        master_entropy.extend_from_slice(ss_recv.as_slice());
    } else {
        master_entropy.extend_from_slice(ss_recv.as_slice());
        master_entropy.extend_from_slice(ss_send.as_slice());
    }
    
    let shared_secret = Zeroizing::new(master_entropy);

    let hkdf = Hkdf::<Sha256>::new(None, shared_secret.as_ref());
    let mut okm = Zeroizing::new([0u8; 128]);
    hkdf.expand(&[], okm.as_mut_slice()).map_err(|e| e.to_string())?;

    let mut k1 = Zeroizing::new([0u8; 32]);
    let mut k2 = Zeroizing::new([0u8; 32]);
    let mut k3 = Zeroizing::new([0u8; 32]);
    let mut k4 = Zeroizing::new([0u8; 32]);
    
    k1.copy_from_slice(&okm[0..32]);
    k2.copy_from_slice(&okm[32..64]);
    k3.copy_from_slice(&okm[64..96]);
    k4.copy_from_slice(&okm[96..128]);

    let (send_text_key, recv_text_key) = if is_host { (k1, k2) } else { (k2, k1) };
    let (send_file_key, recv_file_key) = if is_host { (k3, k4) } else { (k4, k3) };

    password_hash.zeroize();

    let use_aes = local_aes && peer_aes_byte == 1;

    Ok(SessionKeys { send_text_key, recv_text_key, send_file_key, recv_file_key, use_aes })
}

fn get_encryption_key() -> (Zeroizing<[u8; 32]>, bool) {
    print!("\n{} ", "[?] Enter Shared Secret Password (AES-256):".cyan());
    io::stdout().flush().unwrap();

    let mut password = Zeroizing::new(Vec::new());
    let mut b = [0u8; 1];
    loop {
        if io::stdin().read_exact(&mut b).is_err() || b[0] == b'\n' || b[0] == b'\r' {
            break;
        }
        password.push(b[0]);
    }

    let password_str = String::from_utf8_lossy(&password).to_string();
    let is_duress = password_str == "decoy";

    let mut hasher = Sha256::new();
    hasher.update(&password);
    let mut key = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(&hasher.finalize());

    (key, is_duress)
}

fn get_username() -> String {
    print!("{} ", "[?] Enter your Alias / Username:".cyan());
    io::stdout().flush().unwrap();
    let mut name = String::new();
    io::stdin().read_line(&mut name).unwrap();
    let name = name.trim().replace("|", "-");
    if name.is_empty() {
        "anon".to_string()
    } else {
        name
    }
}

fn main() {
    loop {
        print_banner();
        println!("{} Host a secure node", " [1]".cyan());
        println!("{} Connect to a node", " [2]".cyan());
        println!("{} Instructions / Help", " [3]".cyan());
        println!("{} Exit", " [4]".cyan());
        print!("\n{} ", " onyx@root:~#".green().bold());
        io::stdout().flush().unwrap();

        let mut choice = String::new();
        io::stdin().read_line(&mut choice).unwrap();
        let choice = choice.trim();

        if choice == "1" {
            host_node();
            break;
        } else if choice == "2" {
            join_node();
            break;
        } else if choice == "3" {
            print_instructions();
        } else if choice == "4" {
            println!("\n{}", "[!] Terminating Onyx-p2p Protocol.".red());
            std::process::exit(0);
        }
    }
}

fn print_instructions() {
    clear_screen();
    println!("\n{}", "=== ONYX-P2P PROTOCOL MANUAL ===".green().bold());
    println!("\n{}", "[ OVERVIEW ]".yellow());
    println!("Onyx-p2p is a true Peer-to-Peer (P2P) terminal application.");
    println!("All traffic is encrypted using military-grade AES-256 before leaving your computer.");

    println!("\n{}", "[ HOW TO FIND YOUR IP ADDRESS ]".yellow());
    println!("If you are on the same Wi-Fi (Local Network), use your Local IP:");
    println!("{} Open CMD/PowerShell and type: {} (Look for IPv4 Address)", "  Windows:".cyan(), "ipconfig".bold());
    println!("{}   Open Terminal and type: {}", "  macOS:".cyan(), "ipconfig getifaddr en0".bold());
    println!("{}   Open Terminal and type: {} or {}", "  Linux:".cyan(), "hostname -I".bold(), "ip a".bold());
    println!("\nIf you are on different networks (Internet), use your Public IP:");
    println!("  Type this in your terminal: {} (or Google 'What is my IP')", "curl ifconfig.me".bold());

    println!("\n{}", "[ HOW TO HOST ]".yellow());
    println!("1. Select Option [1] and create a Secret Password.");
    println!("2. Give your IP Address and the Password to your peer.");
    println!("3. Wait for the incoming cryptographic handshake.");

    println!("\n{}", "[ HOW TO CONNECT ]".yellow());
    println!("1. Select Option [2] and type the Host's IP Address.");
    println!("2. Enter the exact same Secret Password.");
    println!("3. You are now securely connected.");

    println!("\n{}", "[ FILE TRANSFERS ]".yellow());
    println!("You can send files of unlimited size securely over the P2P stream.");
    println!("To send a file, type: {}", "/send C:\\path\\to\\your\\file.png".bold());

    print!("\n{} ", "Press ENTER to return to the main menu...".cyan());
    io::stdout().flush().unwrap();
    let mut buf = String::new();
    io::stdin().read_line(&mut buf).unwrap();
}

fn host_node() {
    let username = get_username();
    let (password_hash, is_duress) = get_encryption_key();

    if is_duress {
        println!("\n{}", "[*] Initializing Onyx-p2p host on port 9000...".yellow());
        std::thread::sleep(std::time::Duration::from_secs(2));
        println!("{}", "[*] Waiting for incoming connection...".yellow());
        std::thread::sleep(std::time::Duration::from_secs(3));
        println!("[+] Incoming connection from 192.168.1.104");
        println!("{}", "[*] Performing X25519 ECDH Cryptographic Handshake...".yellow());
        std::thread::sleep(std::time::Duration::from_secs(1));
        println!("\n{}", "[+] Handshake successful! Secure connection established.".green().bold());
        run_decoy_session(&username);
        return;
    }

    println!("\n{}", "[*] Initializing Onyx-p2p host on port 9000...".yellow());
    let listener = match TcpListener::bind("0.0.0.0:9000") {
        Ok(l) => l,
        Err(e) => {
            println!("{} {}", "[!] Failed to bind port:".red(), e);
            return;
        }
    };

    println!("{}", "[*] Waiting for incoming connection...".yellow());
    let (conn, _) = match listener.accept() {
        Ok(c) => c,
        Err(e) => {
            println!("{} {}", "[!] Handshake failed:".red(), e);
            return;
        }
    };

    println!("{}", "[*] Performing X25519 ECDH Cryptographic Handshake...".yellow());
    match secure_handshake(&conn, password_hash, true) {
        Ok(keys) => {
            println!("\n{}", "[+] Handshake successful! Secure connection established.".green().bold());
            handle_connection(conn, keys, username);
        }
        Err(e) => {
            println!("\n{} {}", "[!] Handshake failed:".red(), e);
        }
    }
}

fn join_node() {
    print!("\n{} ", "[?] Enter target IP address:".cyan());
    io::stdout().flush().unwrap();
    let mut ip = String::new();
    io::stdin().read_line(&mut ip).unwrap();
    let ip = ip.trim();

    let username = get_username();
    let (password_hash, is_duress) = get_encryption_key();

    if is_duress {
        println!("\n{} {}:9000...", "[*] Attempting connection to".yellow(), ip);
        std::thread::sleep(std::time::Duration::from_secs(2));
        println!("{}", "[*] Performing X25519 ECDH Cryptographic Handshake...".yellow());
        std::thread::sleep(std::time::Duration::from_secs(1));
        println!("\n{}", "[+] Handshake successful! Secure connection established.".green().bold());
        run_decoy_session(&username);
        return;
    }

    println!("\n{} {}:9000...", "[*] Attempting connection to".yellow(), ip);
    let conn = match TcpStream::connect(format!("{}:9000", ip)) {
        Ok(c) => c,
        Err(e) => {
            println!("{} {}", "[!] Connection failed:".red(), e);
            return;
        }
    };

    println!("{}", "[*] Performing X25519 ECDH Cryptographic Handshake...".yellow());
    match secure_handshake(&conn, password_hash, false) {
        Ok(keys) => {
            println!("\n{}", "[+] Handshake successful! Secure connection established.".green().bold());
            handle_connection(conn, keys, username);
        }
        Err(e) => {
            println!("\n{} {}", "[!] Handshake failed:".red(), e);
        }
    }
}

fn handle_connection(conn: TcpStream, keys: SessionKeys, username: String) {
    let conn_read = match conn.try_clone() {
        Ok(c) => c,
        Err(_) => {
            println!("{}", "[!] Error: Operating system failed to duplicate socket handle.".red());
            return;
        }
    };
    let conn_write = conn;

    println!("{}", "------------------------------------------------".cyan());
    println!("{}", " Type your message and press ENTER to send.".cyan());
    println!("{}", " Type '/send <filepath>' to securely transfer a file.".cyan());
    println!("{}", " Type '/ephemeral <seconds>' to enable auto-destructing messages.".cyan());
    println!("{}", " Type '/exit' to drop the connection.".cyan());
    println!("{}\n", "------------------------------------------------".cyan());

    let (accept_tx, accept_rx) = mpsc::channel::<bool>();
    let (file_ack_tx, file_ack_rx) = mpsc::channel::<i64>();

    let send_text_ratchet = Arc::new(Mutex::new(*keys.send_text_key));
    let recv_text_ratchet = Arc::new(Mutex::new(*keys.recv_text_key));
    
    let send_file_ratchet = Arc::new(Mutex::new(*keys.send_file_key));
    let recv_file_ratchet = Arc::new(Mutex::new(*keys.recv_file_key));
    let use_aes = keys.use_aes;

    // Phase 3: Extreme Ephemerality (Anti-Screenshot)
    enable_anti_screenshot();

    if use_aes {
        println!("{}", "[+] Negotiated Cipher: AES-256-GCM (Hardware Accelerated)".green());
    } else {
        println!("{}", "[+] Negotiated Cipher: ChaCha20-Poly1305 (Software Fallback)".yellow());
    }

    let tk_noise = Arc::clone(&send_text_ratchet);
    let conn_write_clone = match conn_write.try_clone() {
        Ok(c) => c,
        Err(_) => {
            println!("{}", "[!] Error: Operating system failed to duplicate socket handle.".red());
            return;
        }
    };
    let write_mutex = Arc::new(Mutex::new(conn_write_clone));
    let cw_noise = Arc::clone(&write_mutex);
    thread::spawn(move || {
        loop {
            let sleep_time = OsRng.gen_range(5..=20);
            thread::sleep(Duration::from_secs(sleep_time));
            if IS_TRANSFERRING.load(Ordering::SeqCst) { continue; }
            let noise_val = OsRng.next_u64();
            let noise_text = format!("/NOISE|{}", noise_val);
            encrypt_and_send_message(&tk_noise, &noise_text, use_aes, &cw_noise);
        }
    });

    let mut rl = Editor::<MyHelper, rustyline::history::DefaultHistory>::new().unwrap();
    rl.bind_sequence(KeyEvent(KeyCode::Esc, Modifiers::NONE), Cmd::Interrupt);
    rl.set_helper(Some(MyHelper));
    let mut ext_printer = rl.create_external_printer().unwrap();

    let tk_read = Arc::clone(&recv_text_ratchet);
    let tk_ack = Arc::clone(&send_text_ratchet);
    let fk_read = Arc::clone(&recv_file_ratchet);
    let cw_read = Arc::clone(&write_mutex);
    let uname_read = username.clone();
    
    thread::spawn(move || {
        let conn_read_tls = conn_read;
        loop {
            let buf = match tls_read_record(&conn_read_tls) {
                Ok(b) => b,
                Err(_) => {
                    let _ = ext_printer.print(format!("\n\n{}\n", "[!] Connection dropped by peer.".red()));
                    std::process::exit(0);
                }
            };
            
            if let Some(dec_msg) = decrypt_message(&tk_read, &buf, use_aes) {
                if dec_msg.starts_with("/NOISE|") {
                    continue;
                } else if dec_msg.starts_with("/SYS_EPHEMERAL|") {
                    let parts: Vec<&str> = dec_msg.split('|').collect();
                    if parts.len() == 2 {
                        let secs: u64 = parts[1].parse().unwrap_or(0);
                        let _ = ext_printer.print(format!("{}\n", format!("[*] Peer enabled Auto-Destruct! Screen will wipe {} seconds after every message.", secs).red().bold()));
                        if secs > 0 {
                            thread::spawn(move || {
                                thread::sleep(Duration::from_secs(secs));
                                print!("\x1B[2J\x1B[1;1H\x1B[3J");
                                let _ = io::stdout().flush();
                            });
                        }
                    }
                    continue;
                } else if dec_msg.starts_with("/SYS_FILE_REQ|") {
                    let parts: Vec<&str> = dec_msg.split('|').collect();
                    if parts.len() < 4 { continue; }
                    let _peer_name = parts[1];
                    let filename_str = parts[2];
                    let filename = std::path::Path::new(filename_str)
                        .file_name()
                        .unwrap_or(std::ffi::OsStr::new("unknown"))
                        .to_str()
                        .unwrap_or("unknown");

                    if filename.is_empty() || filename == "." || filename == ".." {
                        continue;
                    }

                    let filesize: i64 = parts[3].parse().unwrap_or(0);

                    let save_name = format!("downloaded_{}", filename);
                    let current_size = if let Ok(meta) = std::fs::metadata(&save_name) {
                        if meta.len() as i64 >= filesize { 0 } else { meta.len() as i64 }
                    } else { 0 };

                    if current_size > 0 {
                        let _ = ext_printer.print(format!("{} {} (RESUME {} / {} bytes)\n", "[SYSTEM] Incoming file transfer request:".yellow(), filename, current_size, filesize));
                    } else {
                        let _ = ext_printer.print(format!("{} {} ({} bytes)\n", "[SYSTEM] Incoming file transfer request:".yellow(), filename, filesize));
                    }
                    let _ = ext_printer.print(format!("{}\n", "[*] Type '/accept' to download or '/deny' to reject.".yellow()));
                    PENDING_FILE_REQ.store(true, Ordering::SeqCst);

                    let accepted = accept_rx.recv().unwrap_or(false);
                    if accepted {
                        let ack_msg = format!("/SYS_FILE_ACK|YES|{}|{}", uname_read, current_size);
                        encrypt_and_send_message(&tk_ack, &ack_msg, use_aes, &cw_read);

                        let mut outfile = match if current_size > 0 {
                            let _ = ext_printer.print(format!("{}\n", "[*] Resuming chunked AEAD stream to disk...".yellow()));
                            OpenOptions::new().append(true).open(&save_name)
                        } else {
                            let _ = ext_printer.print(format!("{}\n", "[*] Downloading chunked AEAD stream to disk...".yellow()));
                            File::create(&save_name)
                        } {
                            Ok(f) => f,
                            Err(_) => {
                                let _ = ext_printer.print(format!("{}\n", "[!] Error: Could not open file for writing on disk!".red()));
                                let ack_msg = format!("/SYS_FILE_ACK|NO|{}|0", uname_read);
                                encrypt_and_send_message(&tk_ack, &ack_msg, use_aes, &cw_read);
                                continue;
                            }
                        };

                        let mut transfer_success = false;
                        let cipher = CipherEngine::new(&fk_read.lock().unwrap(), use_aes);
                        loop {
                            let payload = match tls_read_record(&conn_read_tls) {
                                Ok(p) => p,
                                Err(_) => break,
                            };
                            if payload.is_empty() {
                                transfer_success = true;
                                break;
                            }

                            if payload.len() < 12 {
                                let _ = ext_printer.print(format!("{}\n", "[!] FATAL: File chunk tampered with mid-transfer (too short)!".red()));
                                break;
                            }

                            let mut nonce_array = [0u8; 12];
                            nonce_array.copy_from_slice(&payload[..12]);
                            let ciphertext = &payload[12..];

                            match cipher.decrypt(&nonce_array, ciphertext) {
                                Some(plaintext) => { 
                                    if outfile.write_all(&plaintext).is_err() {
                                        let _ = ext_printer.print(format!("{}\n", "[!] IO Error: Failed to write chunk to disk!".red()));
                                        break;
                                    }
                                }
                                None => {
                                    let _ = ext_printer.print(format!("{}\n", "[!] FATAL: File chunk tampered with mid-transfer!".red()));
                                    break;
                                }
                            }
                        }
                        if transfer_success {
                            let _ = ext_printer.print(format!("{}\n", "[+] File successfully transferred and authenticated!".green()));
                        } else {
                            let _ = ext_printer.print(format!("{}\n", "[!] File transfer interrupted! (Partial file saved for resume)".red()));
                        }
                    } else {
                        let ack_msg = format!("/SYS_FILE_ACK|NO|{}|0", uname_read);
                        encrypt_and_send_message(&tk_ack, &ack_msg, use_aes, &cw_read);
                        let _ = ext_printer.print(format!("{}\n", "[!] Transfer rejected.".red()));
                    }
                } else if dec_msg.starts_with("/SYS_FILE_ACK|YES|") {
                    let parts: Vec<&str> = dec_msg.split('|').collect();
                    let offset: i64 = if parts.len() >= 4 { parts[3].parse().unwrap_or(0) } else { 0 };
                    let _ = file_ack_tx.send(offset);
                } else if dec_msg.starts_with("/SYS_FILE_ACK|NO|") {
                    let _ = file_ack_tx.send(-1);
                } else {
                    let _ = ext_printer.print(format!("{}\n", dec_msg.yellow()));
                }
            }
        }
    });

    let prompt = format!("{}@onyx:~# ", username);

    loop {
        let readline = rl.readline(&prompt);
        match readline {
            Ok(line) => {
                let text = line.trim();
                if text.is_empty() { continue; }
                
                let _ = rl.add_history_entry(text);

                if text == "/exit" {
                    println!("{}", "[!] Dropping connection.".red());
                    break;
                } else if text == "/accept" {
                    if PENDING_FILE_REQ.load(Ordering::SeqCst) {
                        PENDING_FILE_REQ.store(false, Ordering::SeqCst);
                        let _ = accept_tx.send(true);
                    } else {
                        println!("{}", "[!] No pending file transfer request.".yellow());
                    }
                } else if text == "/deny" {
                    if PENDING_FILE_REQ.load(Ordering::SeqCst) {
                        PENDING_FILE_REQ.store(false, Ordering::SeqCst);
                        let _ = accept_tx.send(false);
                    } else {
                        println!("{}", "[!] No pending file transfer request.".yellow());
                    }
                } else if text.starts_with("/ephemeral ") {
                    let secs: u64 = text.trim_start_matches("/ephemeral ").trim().parse().unwrap_or(0);
                    println!("{}", format!("[*] Auto-Destruct enabled! Messages will vanish in {} seconds.", secs).red().bold());
                    encrypt_and_send_message(&send_text_ratchet, &format!("/SYS_EPHEMERAL|{}", secs), use_aes, &write_mutex);
                } else if text.starts_with("/send ") {
                    let filepath = text.trim_start_matches("/send ").trim();
                    send_file(&write_mutex, &send_text_ratchet, &send_file_ratchet, filepath, &username, &file_ack_rx, use_aes);
                } else {
                    let formatted = format!("[{}] {}", username, text);
                    
                    // Keystroke Cadence Obfuscation: Inject 50-300ms random delay
                    let delay_ms = OsRng.gen_range(50..=300);
                    thread::sleep(Duration::from_millis(delay_ms));

                    encrypt_and_send_message(&send_text_ratchet, &formatted, use_aes, &write_mutex);
                }
            }
            Err(ReadlineError::Interrupted) => {
                // PANIC BUTTON TRIGGERED (Ctrl-C or ESC)
                // 1. Wipe the screen immediately using ANSI escape codes
                print!("\x1B[2J\x1B[1;1H\x1B[3J");
                let _ = io::stdout().flush();
                // 2. Terminate the process (OS reclaims memory; Drop traits zeroize keys)
                std::process::exit(0);
            }
            Err(ReadlineError::Eof) => {
                break;
            }
            Err(_) => break,
        }
    }
}

fn send_file(
    write_mutex: &Arc<Mutex<TcpStream>>,
    text_ratchet: &Arc<Mutex<[u8; 32]>>,
    file_ratchet: &Arc<Mutex<[u8; 32]>>,
    filepath: &str,
    username: &str,
    file_ack_rx: &Receiver<i64>,
    use_aes: bool
) {
    let mut file = match File::open(filepath) {
        Ok(f) => f,
        Err(_) => {
            println!("{}", "[!] Error: File not found or inaccessible.".red());
            return;
        }
    };

    let meta = match file.metadata() {
        Ok(m) => m,
        Err(_) => {
            println!("{}", "[!] Error: Failed to read file metadata.".red());
            return;
        }
    };
    let filesize = meta.len();
    let filepath_path = std::path::Path::new(filepath);
    let filename = match filepath_path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => {
            println!("{}", "[!] Error: Invalid filename.".red());
            return;
        }
    };

    println!("{} {} ({} bytes)...", "[*] Requesting permission to send".yellow(), filename, filesize);

    IS_TRANSFERRING.store(true, Ordering::SeqCst);

    let req_msg = format!("/SYS_FILE_REQ|{}|{}|{}", username, filename, filesize);
    encrypt_and_send_message(text_ratchet, &req_msg, use_aes, write_mutex);

    let offset = file_ack_rx.recv().unwrap_or(-1);
    if offset == -1 {
        println!("{}", "[!] Peer denied the file transfer.".red());
        IS_TRANSFERRING.store(false, Ordering::SeqCst);
        return;
    }

    if offset > 0 {
        println!("{} {}", "[*] Permission granted. Resuming transmission from byte".yellow(), offset);
        use std::io::Seek;
        if file.seek(std::io::SeekFrom::Start(offset as u64)).is_err() {
            println!("{}", "[!] Failed to seek in file.".red());
            IS_TRANSFERRING.store(false, Ordering::SeqCst);
            return;
        }
    } else {
        println!("{}", "[*] Permission granted. Streaming via chunked encryption...".yellow());
    }

    let cipher = CipherEngine::new(&file_ratchet.lock().unwrap(), use_aes);
    let mut buf = [0u8; 16000];

    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let mut nonce_bytes = [0u8; 12];
                OsRng.fill_bytes(&mut nonce_bytes);
                let ciphertext = match cipher.encrypt(&nonce_bytes, &buf[..n]) {
                    Some(c) => c,
                    None => {
                        println!("{}", "[!] Encryption failed mid-transfer.".red());
                        break;
                    }
                };
                
                let mut payload = nonce_bytes.to_vec();
                payload.extend(ciphertext);

                if let Ok(lock) = write_mutex.lock() {
                    let _ = tls_write_record(&*lock, &payload);
                }
            }
            Err(_) => break,
        }
    }
    // End marker
    if let Ok(lock) = write_mutex.lock() {
        let _ = tls_write_record(&*lock, &[]);
    }
    println!("{}", "[+] File transmission complete.".green());
    IS_TRANSFERRING.store(false, Ordering::SeqCst);
}

fn run_decoy_session(username: &str) {
    println!("{}", "[+] Negotiated Cipher: ChaCha20-Poly1305 (Software Fallback)".yellow());
    
    let mut rl = Editor::<MyHelper, rustyline::history::DefaultHistory>::new().unwrap();
    rl.bind_sequence(KeyEvent(KeyCode::Esc, Modifiers::NONE), Cmd::Interrupt);
    rl.set_helper(Some(MyHelper));
    let mut ext_printer = rl.create_external_printer().unwrap();

    let decoy_messages = vec![
        "Hey man, you there?",
        "Did you check the server logs?",
        "Yeah it looks completely normal.",
        "Alright, good. Talk later."
    ];

    thread::spawn(move || {
        for msg in decoy_messages {
            thread::sleep(Duration::from_secs(5));
            let _ = ext_printer.print(format!("[Peer] {}\n", msg));
        }
    });

    let prompt = format!("{}@onyx:~# ", username);
    loop {
        match rl.readline(&prompt) {
            Ok(line) => {
                let text = line.trim();
                if text == "/exit" {
                    println!("{}", "[!] Dropping connection.".red());
                    break;
                }
            }
            Err(ReadlineError::Interrupted) => {
                print!("\x1B[2J\x1B[1;1H\x1B[3J");
                let _ = io::stdout().flush();
                std::process::exit(0);
            }
            Err(_) => break,
        }
    }
}
