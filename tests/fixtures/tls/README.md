# Local TLS fixtures

These certificates and the **public, test-only server private key** are used only
by loopback servers in `tests/tls.rs`. Never use them for a deployed service or
install this CA into a system trust store. The CA private key is not retained.

The RSA-2048 CA and localhost server certificate are valid from September 19,
2026 through September 16, 2036 (UTC). Regenerate them before expiry. The server
certificate has `DNS:localhost` as its only subject alternative name; it has no
IP alternative name, intentionally allowing the wrong-host rejection test.

`ca.pem` is a PEM trust anchor, `server.der` is a DER X.509 certificate, and
`server-key.der` is an unencrypted DER PKCS#8 test key. OpenSSL is needed only to
regenerate fixtures, not to run tests. From the repository root:

```sh
fixture_tmp=$(mktemp -d)
openssl req -x509 -newkey rsa:2048 -nodes \
  -keyout "$fixture_tmp/ca.key" -out tests/fixtures/tls/ca.pem -days 3650 \
  -subj '/CN=TypeSafe test CA' \
  -addext 'basicConstraints=critical,CA:TRUE' \
  -addext 'keyUsage=critical,keyCertSign,cRLSign'
openssl req -newkey rsa:2048 -nodes \
  -keyout "$fixture_tmp/server.key" -out "$fixture_tmp/server.csr" \
  -subj '/CN=localhost'
cat > "$fixture_tmp/server.ext" <<'EOF'
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
subjectAltName=DNS:localhost
EOF
openssl x509 -req -in "$fixture_tmp/server.csr" \
  -CA tests/fixtures/tls/ca.pem -CAkey "$fixture_tmp/ca.key" \
  -set_serial 2 -days 3650 -extfile "$fixture_tmp/server.ext" \
  -outform DER -out tests/fixtures/tls/server.der
openssl pkcs8 -topk8 -nocrypt -in "$fixture_tmp/server.key" \
  -outform DER -out tests/fixtures/tls/server-key.der
rm -r "$fixture_tmp"
```

Update the validity dates above after regeneration and run `cargo test --locked
--test tls` on Linux to verify both trust acceptance and hostname rejection.
