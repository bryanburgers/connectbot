## Self-Signed CA

Create a CA that can be used to verify that all of the keys come from the right place.

    openssl genrsa -out ca.key 4096
    openssl req -new -x509 -days 36500 -key ca.key -out ca.crt

This key should be kept safe, because if anybody else has it, they can impersonate.

## Server Key

Create the key for the server side of the connection. We should need only one.

    openssl genrsa -out server.key 4096
    openssl req -new -key server.key -out server.csr
    
    openssl x509 -req -in server.csr -CA ca.crt -CAkey ca.key -CAcreateserial -out server.crt -days 36500 -sha256
    cat server.crt server.key > server.pem
    
    openssl x509 -in server.crt -text -noout


## Client Key

Create a key for each client that will connect. We should have one per device.

    openssl genrsa -out client.key 4096
    openssl req -new -key client.key -out client.csr
    
    openssl x509 -req -in client.csr -CA ca.crt -CAkey ca.key -CAcreateserial -out client.crt -days 36500 -sha256
    cat client.crt client.key > client.pem
    
    openssl x509 -in client.crt -text -noout
