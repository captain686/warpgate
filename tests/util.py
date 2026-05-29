import logging
import os
from pathlib import Path
import requests
import socket
import subprocess
import tempfile
import threading
import time
import typing

import paramiko


allocated_ports = set()

mysql_client_ssl_opt = "--ssl"
mysql_client_opts = []
if "GITHUB_ACTION" in os.environ:
    # Github uses MySQL instead of MariaDB
    mysql_client_ssl_opt = "--ssl-mode=REQUIRED"
    mysql_client_opts = ["--enable-cleartext-plugin"]


def alloc_port():
    while True:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.bind(("127.0.0.1", 0))
            sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            port = sock.getsockname()[1]
        if port in allocated_ports:
            continue
        allocated_ports.add(port)
        return port


def _wait_timeout(fn, msg, timeout=60):
    error: list[BaseException] = []

    def runner():
        try:
            fn()
        except BaseException as exc:  # noqa: BLE001
            error.append(exc)

    t = threading.Thread(target=runner, daemon=True)
    t.start()
    t.join(timeout=timeout)
    if t.is_alive():
        raise Exception(msg)
    if error:
        raise error[0]


def wait_port(port, recv=True, timeout=60, for_process: subprocess.Popen = None, connect_timeout=5, read_timeout=5):
    logging.debug(f"Waiting for port {port}")

    def wait():
        while True:
            try:
                s = socket.create_connection(("localhost", port), timeout=connect_timeout)
                if recv:
                    s.settimeout(read_timeout)
                    if not s.recv(100):
                        raise OSError("Port is open but not responding")
                s.close()
                logging.debug(f"Port {port} is up")
                return
            except socket.error:
                if for_process:
                    try:
                        for_process.wait(timeout=0.1)
                        raise RuntimeError("Process exited while waiting for port")
                    except subprocess.TimeoutExpired:
                        continue
                else:
                    time.sleep(0.1)

    _wait_timeout(wait, f"Port {port} is not up", timeout=timeout)


def wait_mysql_port(port):
    logging.debug(f"Waiting for MySQL port {port}")

    def wait():
        while True:
            try:
                with socket.create_connection(("localhost", port), timeout=5) as conn:
                    conn.settimeout(5)
                    if not conn.recv(64):
                        raise OSError("MySQL port is open but not responding")
                logging.debug(f"Port {port} is up")
                break
            except OSError:
                time.sleep(1)
                continue

    _wait_timeout(wait, f"Port {port} is not up", timeout=60)


def create_ticket(url, username, target_name):
    session = requests.Session()
    session.verify = False
    response = session.post(
        f"{url}/@warpgate/api/auth/login",
        json={
            "username": "admin",
            "password": "123",
        },
    )
    assert response.status_code // 100 == 2
    response = session.post(
        f"{url}/@warpgate/admin/api/tickets",
        json={
            "username": username,
            "target_name": target_name,
        },
    )
    assert response.status_code == 201
    return response.json()["secret"]


def _load_private_key(key_path: str | Path) -> paramiko.PKey:
    key_path = str(key_path)
    loaders: list[typing.Callable[[str], paramiko.PKey]] = [
        paramiko.Ed25519Key.from_private_key_file,
        paramiko.RSAKey.from_private_key_file,
        paramiko.ECDSAKey.from_private_key_file,
    ]
    dss_key = getattr(paramiko, "DSSKey", None)
    if dss_key is not None:
        loaders.append(dss_key.from_private_key_file)
    last_error: Exception | None = None
    for loader in loaders:
        try:
            return loader(key_path)
        except Exception as error:  # noqa: BLE001
            last_error = error
    raise paramiko.SSHException(
        f"Could not load SSH private key from {key_path}: {last_error}"
    )


def ssh_exec_command_with_public_key(
    host: str,
    port: int,
    username: str,
    private_key_path: str | Path,
    command: str,
    *,
    timeout: float = 10.0,
    otp_code: str | None = None,
) -> tuple[int, bytes, bytes]:
    """Execute command through OpenSSH client using pubkey and optional OTP.

    Uses the local `ssh` binary to avoid `expect`/`sshpass` runtime dependencies
    while still supporting keyboard-interactive OTP prompts.
    """
    private_key_path = str(private_key_path)
    ssh_args = [
        "ssh",
        "-o",
        "IdentitiesOnly=yes",
        "-o",
        "StrictHostKeyChecking=no",
        "-o",
        "UserKnownHostsFile=/dev/null",
        "-o",
        "LogLevel=ERROR",
        "-o",
        "PreferredAuthentications=publickey,keyboard-interactive",
        "-o",
        "NumberOfPasswordPrompts=1",
        "-o",
        f"IdentityFile={private_key_path}",
        "-p",
        str(port),
        f"{username}@{host}",
        command,
    ]

    askpass_path: Path | None = None
    env = os.environ.copy()
    if otp_code is not None:
        with tempfile.NamedTemporaryFile(
            mode="w",
            prefix="warpgate-ssh-askpass-",
            suffix=".sh",
            delete=False,
        ) as askpass_file:
            askpass_file.write("#!/bin/sh\nprintf '%s\\n' \"$WARPGATE_TEST_SSHPASS\"\n")
            askpass_path = Path(askpass_file.name)
        askpass_path.chmod(0o700)
        env.update(
            {
                "WARPGATE_TEST_SSHPASS": otp_code,
                "SSH_ASKPASS": str(askpass_path),
                "SSH_ASKPASS_REQUIRE": "force",
                "DISPLAY": "warpgate-test:0",
            }
        )

    process = subprocess.Popen(
        ssh_args,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
        start_new_session=True,
    )
    try:
        stdout, stderr = process.communicate(timeout=timeout)
    except subprocess.TimeoutExpired as error:
        process.kill()
        stdout, stderr = process.communicate()
        raise paramiko.AuthenticationException(
            stderr.decode(errors="replace")
            or "SSH authentication timed out"
        ) from error
    finally:
        if askpass_path is not None:
            askpass_path.unlink(missing_ok=True)

    status = process.returncode
    if status != 0:
        raise paramiko.AuthenticationException(
            stderr.decode(errors="replace") or f"SSH command failed with exit code {status}"
        )

    return status, stdout, stderr
