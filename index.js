#!/usr/bin/env node

const { spawnSync } = require('child_process');
const path = require('path');
const fs = require('fs');

/**
 * Onyx-p2p NPM Wrapper
 * This acts as a proxy bridge between the Node ecosystem (npx onyx-p2p)
 * and the Onyx-p2p Rust core engine.
 */

function getBinaryPath() {
    const isWindows = process.platform === 'win32';
    const ext = isWindows ? '.exe' : '';
    const binPath = path.join(__dirname, 'bin', `onyx-p2p${ext}`);
    
    if (fs.existsSync(binPath)) return binPath;
    
    // Fallback to local cargo build if developing locally
    let devBin = path.join(__dirname, 'target', 'release', `onyx-p2p${ext}`);
    if (fs.existsSync(devBin)) return devBin;
    
    return null;
}

function run() {
    const args = process.argv.slice(2);
    const binary = getBinaryPath();

    if (!binary) {
        console.error("❌ Onyx-p2p Rust binary not found.");
        console.error("Please compile the Rust core first by running: cargo build --release");
        process.exit(1);
    }

    const result = spawnSync(binary, args, { stdio: 'inherit' });

    if (result.error) {
        console.error(`❌ Failed to execute Onyx-p2p Rust engine: ${result.error.message}`);
        process.exit(1);
    }

    process.exit(result.status ?? 0);
}

run();
