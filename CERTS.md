## Self-Signed CA

Create a CA that can be used to verify that all of the keys come from the right place.

    openssl genrsa -out ca.key 4096
    openssl req -new -x509 -days 36500 -key ca.key -out ca.crt -nodes -subj "/CN=comms-ca"

This key should be kept safe, because if anybody else has it, they can impersonate.

## Server Key

Create the key for the server side of the connection. We should need only one.

    openssl genrsa -out server.key 4096
    openssl req -new -key server.key -out server.csr -nodes -subj "/CN=comms-server"
    
    openssl x509 -req -in server.csr -CA ca.crt -CAkey ca.key -CAcreateserial -out server.crt -days 36500 -sha256
    cat server.crt ca.crt > server.pem
    
    openssl x509 -in server.crt -text -noout

## Server Key (redux)

    openssl req -new -out server2.csr -key server.key -nodes -config server2.cnf
    openssl x509 -req -in server2.csr -CA ca.crt -CAkey ca.key -CAcreateserial -out server2.crt -days 36500 -sha256 -extfile server2.cnf -extensions req_ext

```
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
DNS.1   = comms-server
```

## Client Key

Create a key for each client that will connect. We should have one per device.

    openssl genrsa -out client.key 4096
    openssl req -new -key client.key -out client.csr
    
    openssl x509 -req -in client.csr -CA ca.crt -CAkey ca.key -CAcreateserial -out client.crt -days 36500 -sha256
    cat client.crt ca.crt > client.pem
    
    openssl x509 -in client.crt -text -noout

## Client Key (redux)

    openssl genrsa -out client.key 4096
    openssl req -new -out client2.csr -key client.key -nodes -config client2.cnf

    openssl x509 -req -in client2.csr -CA ca.crt -CAkey ca.key -CAcreateserial -out client2.crt -days 36500 -sha256 -extfile client2.cnf -extensions req_ext
