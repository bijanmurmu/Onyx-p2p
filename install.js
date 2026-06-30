const fs = require('fs');
const path = require('path');
const https = require('https');
const { execSync } = require('child_process');

const version = require('./package.json').version;
const platform = process.platform;
const arch = process.arch;

let binName = '';
if (platform === 'win32') {
    binName = arch === 'arm64' ? 'onyx-p2p-windows-arm64.exe' : 'onyx-p2p-windows-x64.exe';
} else if (platform === 'darwin') {
    binName = arch === 'arm64' ? 'onyx-p2p-macos-arm64' : 'onyx-p2p-macos-x64';
} else if (platform === 'linux') {
    binName = arch === 'arm64' ? 'onyx-p2p-linux-arm64' : 'onyx-p2p-linux-x64';
} else {
    console.error(`❌ Unsupported platform: ${platform}`);
    process.exit(1);
}

const url = `https://github.com/bijanmurmu/Onyx-p2p/releases/download/v${version}/${binName}`;
const binDir = path.join(__dirname, 'bin');
if (!fs.existsSync(binDir)) {
    fs.mkdirSync(binDir);
}

const destPath = path.join(binDir, platform === 'win32' ? 'onyx-p2p.exe' : 'onyx-p2p');

console.log(`\n🚀 Downloading Onyx-p2p Rust engine v${version} for ${platform}...`);

function downloadFile(url, dest) {
    return new Promise((resolve, reject) => {
        const file = fs.createWriteStream(dest);
        https.get(url, (response) => {
            if (response.statusCode === 301 || response.statusCode === 302) {
                downloadFile(response.headers.location, dest).then(resolve).catch(reject);
            } else if (response.statusCode === 200) {
                response.pipe(file);
                file.on('finish', () => {
                    file.close();
                    if (platform !== 'win32') {
                        execSync(`chmod +x "${dest}"`);
                    }
                    console.log("✅ Onyx-p2p successfully installed!\n");
                    resolve();
                });
            } else {
                reject(new Error(`Failed to download binary: HTTP ${response.statusCode}`));
            }
        }).on('error', (err) => {
            fs.unlink(dest, () => {});
            reject(err);
        });
    });
}

downloadFile(url, destPath).catch(err => {
    console.error(`❌ Installation failed: ${err.message}`);
    process.exit(1);
});
