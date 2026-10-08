"""Disposable TLS-policy fixtures and a verified low-strength server oracle.

Never changes a system/user trust store. Low-strength crypto is confined to this
test server and its CA/hostname-verifying oracle, so a client rejection cannot
be mistaken for a server that was unable to offer the weak certificate chain.
"""

import argparse
import json
import os
import socket
import ssl
import subprocess
import threading
from pathlib import Path

# (server key, issuing CA, trusted root). Roots are never sent by the server.
CHAINS = {
    "strong": ("leaf", "root", "root"),
    "strong-ec": ("ec-leaf", "ec-root", "ec-root"),
    "strong-pss": ("leaf", "root", "root"),
    "strong-pss-key": ("leaf", "pss-root", "pss-root"),
    "weak-leaf": ("weak-leaf", "root", "root"),
    "weak-intermediate": ("leaf", "intermediate", "root"),
    "weak-root": ("leaf", "weak-root", "weak-root"),
    "weak-2047-root": ("leaf", "root-2047", "root-2047"),
    "weak-ec-intermediate": ("ec-leaf", "ec-intermediate", "ec-root"),
    "weak-ec-root": ("ec-leaf", "weak-ec-root", "weak-ec-root"),
}


def openssl(*args):
    subprocess.run(
        [os.environ.get("DESKTOP_TLS_OPENSSL", "openssl"), *map(str, args)],
        check=True,
        capture_output=True,
        timeout=30,
    )


def prepare(directory):
    version = subprocess.check_output(
        [os.environ.get("DESKTOP_TLS_OPENSSL", "openssl"), "version"],
        text=True,
        timeout=10,
    ).strip()
    if not version.startswith("OpenSSL 3."):
        raise RuntimeError(
            f"TLS fixtures require the selected OpenSSL 3 CLI; got {version!r}"
        )
    if ssl.OPENSSL_VERSION_INFO[0] != 3:
        raise RuntimeError(
            f"TLS fixture Python must use OpenSSL 3; got {ssl.OPENSSL_VERSION!r}"
        )
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    os.umask(0o077)
    ca_ext = directory / "ca.ext"
    ca_ext.write_text(
        "basicConstraints=critical,CA:TRUE\nkeyUsage=critical,keyCertSign,cRLSign\n"
    )
    leaf_ext = directory / "leaf.ext"
    leaf_ext.write_text(
        "basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\n"
        "extendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost\n"
    )
    for name, algorithm, option in [
        ("root", "RSA", "rsa_keygen_bits:2048"),
        ("weak-root", "RSA", "rsa_keygen_bits:1024"),
        ("root-2047", "RSA", "rsa_keygen_bits:2047"),
        ("intermediate", "RSA", "rsa_keygen_bits:1024"),
        ("leaf", "RSA", "rsa_keygen_bits:2048"),
        ("weak-leaf", "RSA", "rsa_keygen_bits:1024"),
        ("ec-root", "EC", "ec_paramgen_curve:prime256v1"),
        ("weak-ec-root", "EC", "ec_paramgen_curve:secp224r1"),
        ("ec-intermediate", "EC", "ec_paramgen_curve:secp224r1"),
        ("ec-leaf", "EC", "ec_paramgen_curve:prime256v1"),
        ("pss-root", "RSA-PSS", "rsa_keygen_bits:2048"),
    ]:
        openssl(
            "genpkey",
            "-algorithm",
            algorithm,
            "-pkeyopt",
            option,
            "-out",
            directory / f"{name}.key",
        )
        openssl(
            "req",
            "-new",
            "-key",
            directory / f"{name}.key",
            "-subj",
            f"/CN=Disposable TLS policy {name}",
            "-out",
            directory / f"{name}.csr",
        )
    for name in (
        "root",
        "weak-root",
        "root-2047",
        "ec-root",
        "weak-ec-root",
        "pss-root",
    ):
        openssl(
            "x509",
            "-req",
            "-in",
            directory / f"{name}.csr",
            "-signkey",
            directory / f"{name}.key",
            "-days",
            "2",
            "-sha256",
            "-extfile",
            ca_ext,
            "-out",
            directory / f"{name}.pem",
        )
    for name, issuer in [("intermediate", "root"), ("ec-intermediate", "ec-root")]:
        openssl(
            "x509",
            "-req",
            "-in",
            directory / f"{name}.csr",
            "-CA",
            directory / f"{issuer}.pem",
            "-CAkey",
            directory / f"{issuer}.key",
            "-set_serial",
            "2",
            "-days",
            "2",
            "-sha256",
            "-extfile",
            ca_ext,
            "-out",
            directory / f"{name}.pem",
        )
    for name, (leaf, issuer, root) in CHAINS.items():
        cert = directory / f"{name}-server.pem"
        openssl(
            "x509",
            "-req",
            "-in",
            directory / f"{leaf}.csr",
            "-CA",
            directory / f"{issuer}.pem",
            "-CAkey",
            directory / f"{issuer}.key",
            "-set_serial",
            "3",
            "-days",
            "2",
            "-sha256",
            "-extfile",
            leaf_ext,
            "-out",
            cert,
            *(["-sigopt", "rsa_padding_mode:pss"] if name == "strong-pss" else []),
        )
        if issuer != root:
            cert.write_bytes(
                cert.read_bytes() + (directory / f"{issuer}.pem").read_bytes()
            )
    print(
        json.dumps(
            {
                "prepared": True,
                "openssl": os.environ.get("DESKTOP_TLS_OPENSSL", "openssl"),
            }
        )
    )


def serve(directory, case):
    certificate_case = case if case in CHAINS else "strong"
    key_name, _, root_name = CHAINS[certificate_case]
    root = directory / f"{root_name}.pem"
    key = directory / f"{key_name}.key"
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.set_ciphers("DEFAULT:@SECLEVEL=0")
    context.load_cert_chain(directory / f"{certificate_case}-server.pem", key)

    def accept_one(listener):
        connection, _ = listener.accept()
        with connection:
            connection.settimeout(15)
            try:
                secured = context.wrap_socket(connection, server_side=True)
            except ssl.SSLError as error:
                return {
                    "phase": "handshake",
                    "bytes": 0,
                    "request": "",
                    "error": str(error),
                }
            data = b""
            try:
                with secured:
                    while b"\r\n\r\n" not in data and len(data) < 16384:
                        chunk = secured.recv(4096)
                        if not chunk:
                            break
                        data += chunk
                    secured.sendall(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK"
                    )
                    return {
                        "phase": "application",
                        "version": secured.version(),
                        "bytes": len(data),
                        "request": data.decode("ascii"),
                    }
            except (OSError, UnicodeError) as error:
                # A completed handshake followed by I/O failure is not proof
                # that the client rejected the certificate. Preserve evidence.
                return {
                    "phase": "application-error",
                    "bytes": len(data),
                    "request": data.decode("ascii", errors="replace"),
                    "error": str(error),
                }

    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        listener.listen(2)
        listener.settimeout(20)
        port = listener.getsockname()[1]
        result = {}

        def oracle_server():
            try:
                result.update(accept_one(listener))
            except (OSError, UnicodeError) as error:
                result.update(phase="oracle-server-error", error=str(error))

        thread = threading.Thread(target=oracle_server, daemon=True)
        thread.start()
        oracle = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
        oracle.minimum_version = ssl.TLSVersion.TLSv1_2
        oracle.set_ciphers("DEFAULT:@SECLEVEL=0")
        oracle.load_verify_locations(cafile=root)
        # CERT_REQUIRED and check_hostname remain enabled. Only the disposable
        # oracle's minimum crypto strength is lowered, never the tested client.
        assert oracle.verify_mode == ssl.CERT_REQUIRED and oracle.check_hostname
        with (
            socket.create_connection(("127.0.0.1", port), timeout=15) as connection,
            oracle.wrap_socket(connection, server_hostname="localhost") as secured,
        ):
            secured.sendall(b"GET /oracle HTTP/1.1\r\nHost: localhost\r\n\r\n")
            response = secured.recv(4096)
            assert response.startswith(b"HTTP/1.1 200 OK")
        thread.join(timeout=20)
        assert not thread.is_alive() and result.get("phase") == "application", result
        print(
            json.dumps(
                {
                    "port": port,
                    "root": str(root),
                    "oracle": result,
                    "case": case,
                    "openssl": ssl.OPENSSL_VERSION,
                    "oracle_policy": {
                        "verify_mode": "CERT_REQUIRED",
                        "check_hostname": oracle.check_hostname,
                        "security_level": oracle.security_level,
                    },
                }
            ),
            flush=True,
        )
        print(json.dumps(accept_one(listener)), flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("prepare", "serve"))
    parser.add_argument("directory", type=Path)
    parser.add_argument(
        "--case",
        choices=(*CHAINS, "untrusted", "wrong-host"),
    )
    args = parser.parse_args()
    if args.mode == "prepare":
        prepare(args.directory)
    else:
        if args.case is None:
            parser.error("serve requires --case")
        serve(args.directory, args.case)
