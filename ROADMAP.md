# 🌑 Onyx-p2p Protocol: The Endgame Roadmap

The goal of Onyx-p2p is to become the absolute most secure, zero-trust, memory-safe P2P communication tool on the planet. With our Version 1.0 architecture fully realized (PQC, DPI Evasion, Memory Wiping, Double Ratchet), the foundation is perfect.

This roadmap represents the **absolute endgame**. Once these final architectural pillars are implemented, Onyx-p2p will reach its final evolutionary state, with literally nothing left to upgrade.

---

---

### 🚫 Banned Features (Known Limitations)
The following features have been **explicitly evaluated and banned** from the roadmap to preserve Onyx-p2p's extreme performance, security, and peer-to-peer purity:

* **Hardware Enclaves (Intel SGX / YubiKey)**: Relying on proprietary hardware (like Intel SGX) introduces massive manufacturer backdoors and known extraction exploits. Onyx-p2p will remain 100% pure software mathematics to guarantee zero trust.
* **Group Swarms (CRDT Mesh)**: 1-on-1 sockets guarantee mathematically perfect forward secrecy. Introducing multi-peer mesh routing destroys this and leaks connection metadata. Onyx-p2p is strictly 1-on-1 forever.
* **Native Desktop GUI (Tauri / Electron)**: GUIs introduce massive web-vulnerability attack surfaces (XSS, CSS injection). Onyx-p2p will remain a pure Rust Terminal UI to maintain a zero-bloat, mathematically minimal attack surface.
* **Tor / Onion Routing**: Routing traffic through multiple external Tor relays introduces extreme latency and destroys our gigabit file transfer speeds. IP exposure is an accepted known limitation to maintain peer-to-peer perfection.
* **Centralized Relays/Servers**: We will never implement a server to hold offline messages. If both users are not online simultaneously, the connection simply cannot happen.
