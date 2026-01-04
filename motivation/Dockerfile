FROM rust

RUN apt-get update && apt-get install -y curl npm tcpdump tshark && rm -rf /var/lib/apt/lists/*

# Install Node.js 25
RUN curl -fsSL https://deb.nodesource.com/setup_25.x | bash - && apt-get install -y nodejs && rm -rf /var/lib/apt/lists/*

RUN cat > reqw13.rs << 'EOF'
#!/usr/bin/env -S cargo +nightly -Zscript
---cargo
[dependencies]
reqwest = { version = "0.13", features = ["hickory-dns"] }
tokio = { version = "1", features = ["full"] }
[profile.dev]
opt-level = 3
debug-assertions = false
overflow-checks = false
debug = false
lto = true
incremental = false
codegen-units = 16
---
fn main() {}
EOF
RUN chmod +x reqw13.rs
RUN ./reqw13.rs

COPY package.json package-lock.json .
RUN npm ci

COPY native-fetch.js node-fetch.js faith.mjs .
COPY reqw13.rs .
RUN chmod +x reqw13.rs
RUN ./reqw13.rs
