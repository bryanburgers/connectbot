## Self-Signed CA

Create a CA that can be used to verify that all of the keys come from the right place.

```
openssl genrsa -out ca.key 4096
openssl req -new -x509 -days 36500 -key ca.key -out ca.crt -nodes -subj "/CN=connectbot-ca"
```

This key should be kept safe, because if anybody else has it, they can impersonate.


## Server Key

Create the key for the server side of the connection. We should need only one.

```
openssl genrsa -out server.key 4096
openssl pkcs8 -topk8 -inform PEM -outform PEM -nocrypt -in server.key -out server-private.pem
```

Now we need to create the certificate request that includes a [DNS Subject
Alternative Name][dnsalt]. `openssl` doesn't make this easy, so we need to
create a config file.

```
cat >server.cnf <<EOF
[ req ]
default_bits        = 4096
distinguished_name  = req_distinguished_name
req_extensions      = req_ext
[ req_distinguished_name ]
countryName                 = Country Name (2 letter code)
stateOrProvinceName         = State or Province Name (full name)
localityName               = Locality Name (eg, city)
organizationName           = Organization Name (eg, company)
commonName                 = Common Name (e.g. server FQDN or YOUR name)
[ req_ext ]
subjectAltName = @alt_names
[alt_names]
DNS.1   = connectbot-server
EOF
```

Now create the cert and sign with the CA.

```
openssl req -new -out server.csr -key server.key -nodes -config server.cnf -subj "/CN=connectbot-server"
openssl x509 -req -in server.csr -CA ca.crt -CAkey ca.key -CAcreateserial -out server.crt -days 36500 -sha256 -extfile server.cnf -extensions req_ext
cat server.crt ca.crt > server.pem
```

[dnsalt]: https://support.dnsimple.com/articles/what-is-common-name/#common-name-vs-subject-alternative-name
