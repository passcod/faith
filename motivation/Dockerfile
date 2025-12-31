FROM rust

RUN apt-get update && apt-get install -y curl npm tcpdump tshark && rm -rf /var/lib/apt/lists/*

# Install Node.js 25
RUN curl -fsSL https://deb.nodesource.com/setup_25.x | bash - && apt-get install -y nodejs && rm -rf /var/lib/apt/lists/*

RUN cat > minimal.rs << 'EOF'
#!/usr/bin/env -S cargo +nightly -Zscript
---cargo
[dependencies]
reqwest = { version = "0.13", features = ["hickory-dns"] }
tokio = { version = "1", features = ["full"] }
---
fn main() {}
EOF
RUN chmod +x minimal.rs
RUN ./minimal.rs

COPY package.json package-lock.json .
RUN npm ci

COPY native-fetch.js node-fetch.js faith.js .
COPY minimal.rs .
RUN chmod +x minimal.rs
RUN ./minimal.rs
