import base64
import json
import ssl
import time
from pathlib import Path
from uuid import uuid4

import requests
from websocket import WebSocketConnectionClosedException, WebSocketTimeoutException
from websocket import create_connection

from .api_client import admin_client, sdk
from .conftest import ProcessManager, WarpgateProcess
from .util import wait_port


class TestWebSsh:
    def test_session_lifecycle(
        self,
        processes: ProcessManager,
        wg_c_ed25519_pubkey: Path,
        timeout,
        shared_wg: WarpgateProcess,
    ):
        ssh_port = processes.start_ssh_server(
            trusted_keys=[wg_c_ed25519_pubkey.read_text()]
        )
        wait_port(ssh_port)

        url = f"https://localhost:{shared_wg.http_port}"
        with admin_client(url) as api:
            role = api.create_role(sdk.RoleDataRequest(name=f"role-{uuid4()}"))
            user = api.create_user(sdk.CreateUserRequest(username=f"user-{uuid4()}"))
            user_id = getattr(user, "id", user.user.id)
            username = getattr(user, "username", user.user.username)
            api.create_password_credential(
                user_id, sdk.NewPasswordCredential(password="123")
            )
            api.add_user_role(user_id, role.id)
            ssh_target = api.create_target(
                sdk.TargetDataRequest(
                    name=f"ssh-{uuid4()}",
                    options=sdk.TargetOptions(
                        sdk.TargetOptionsTargetSSHOptions(
                            kind="Ssh",
                            host="localhost",
                            port=ssh_port,
                            username=processes.ssh_target_username,
                            auth=sdk.SSHTargetAuth(
                                sdk.SSHTargetAuthSshTargetPublicKeyAuth(
                                    kind="PublicKey"
                                )
                            ),
                        )
                    ),
                )
            )
            api.add_target_role(ssh_target.id, role.id)

        # Log in as the user
        http = requests.Session()
        http.verify = False
        resp = http.post(
            f"{url}/@warpgate/api/auth/login",
            json={"username": username, "password": "123"},
        )
        assert resp.status_code // 100 == 2

        # Create a web SSH session
        resp = http.post(
            f"{url}/@warpgate/api/web-ssh/sessions",
            json={"target_id": str(ssh_target.id)},
        )
        assert resp.status_code == 201, resp.text
        session_id = resp.json()["session_id"]

        # Verify session info is retrievable
        resp = http.get(f"{url}/@warpgate/api/web-ssh/sessions/{session_id}")
        assert resp.status_code == 200
        assert resp.json()["target_name"] == ssh_target.name

        # Connect via WebSocket
        cookie = "; ".join(f"{k}={v}" for k, v in http.cookies.get_dict().items())
        ws = create_connection(
            f"wss://localhost:{shared_wg.http_port}/@warpgate/api/web-ssh/sessions/{session_id}/stream",
            cookie=cookie,
            sslopt={"cert_reqs": ssl.CERT_NONE},
        )
        try:
            # Request a shell channel
            ws.send(json.dumps({"type": "open_channel", "cols": 80, "rows": 24}))

            deadline = time.time() + timeout
            channel_id = None
            while time.time() < deadline:
                msg = json.loads(ws.recv())
                if msg["type"] == "channel_opened":
                    channel_id = msg["channel_id"]
                    break
                if msg["type"] == "error":
                    raise AssertionError(f"SSH error: {msg['message']}")
            else:
                raise TimeoutError("Did not receive channel_opened message in time")

            assert channel_id is not None, "Did not receive channel_opened"

            # Send a command and collect output until the marker appears
            cmd = "echo webssh_test\n"
            ws.send(
                json.dumps(
                    {
                        "type": "input",
                        "channel_id": channel_id,
                        "data": base64.b64encode(cmd.encode()).decode(),
                    }
                )
            )

            output = ""
            while time.time() < deadline:
                msg = json.loads(ws.recv())
                if msg["type"] == "output" and msg["channel_id"] == channel_id:
                    output += base64.b64decode(msg["data"]).decode(errors="replace")
                    if "webssh_test" in output:
                        break
                elif msg["type"] == "error":
                    raise AssertionError(f"SSH error: {msg['message']}")
            else:
                raise TimeoutError("Did not receive expected output in time")

            # Close the channel
            ws.send(json.dumps({"type": "close_channel", "channel_id": channel_id}))
        finally:
            ws.close()

        # Delete the session
        resp = http.delete(f"{url}/@warpgate/api/web-ssh/sessions/{session_id}")
        assert resp.status_code == 204

        # Session should be gone
        resp = http.get(f"{url}/@warpgate/api/web-ssh/sessions/{session_id}")
        assert resp.status_code == 404

    def test_target_auth_error_is_reported(
        self,
        processes: ProcessManager,
        timeout,
        shared_wg: WarpgateProcess,
    ):
        ssh_port = processes.start_ssh_server(trusted_keys=[])
        wait_port(ssh_port)

        url = f"https://localhost:{shared_wg.http_port}"
        with admin_client(url) as api:
            role = api.create_role(sdk.RoleDataRequest(name=f"role-{uuid4()}"))
            user = api.create_user(sdk.CreateUserRequest(username=f"user-{uuid4()}"))
            user_id = getattr(user, "id", user.user.id)
            username = getattr(user, "username", user.user.username)
            api.create_password_credential(
                user_id, sdk.NewPasswordCredential(password="123")
            )
            api.add_user_role(user_id, role.id)
            ssh_target = api.create_target(
                sdk.TargetDataRequest(
                    name=f"ssh-{uuid4()}",
                    options=sdk.TargetOptions(
                        sdk.TargetOptionsTargetSSHOptions(
                            kind="Ssh",
                            host="localhost",
                            port=ssh_port,
                            username=processes.ssh_target_username,
                            auth=sdk.SSHTargetAuth(
                                sdk.SSHTargetAuthSshTargetPublicKeyAuth(
                                    kind="PublicKey"
                                )
                            ),
                        )
                    ),
                )
            )
            api.add_target_role(ssh_target.id, role.id)

        http = requests.Session()
        http.verify = False
        resp = http.post(
            f"{url}/@warpgate/api/auth/login",
            json={"username": username, "password": "123"},
        )
        assert resp.status_code // 100 == 2

        resp = http.post(
            f"{url}/@warpgate/api/web-ssh/sessions",
            json={"target_id": str(ssh_target.id)},
        )
        assert resp.status_code == 201, resp.text
        session_id = resp.json()["session_id"]

        deadline = time.time() + timeout
        session_info = None
        while time.time() < deadline:
            resp = http.get(f"{url}/@warpgate/api/web-ssh/sessions/{session_id}")
            assert resp.status_code == 200
            session_info = resp.json()
            if session_info.get("closed") and session_info.get("last_error"):
                break
            time.sleep(0.1)
        else:
            raise TimeoutError("Did not receive WebSSH session error in time")

        assert "Public key authentication was rejected" in session_info["last_error"]

        cookie = "; ".join(f"{k}={v}" for k, v in http.cookies.get_dict().items())
        ws = create_connection(
            f"wss://localhost:{shared_wg.http_port}/@warpgate/api/web-ssh/sessions/{session_id}/stream",
            cookie=cookie,
            sslopt={"cert_reqs": ssl.CERT_NONE},
        )
        ws.settimeout(0.5)
        error_message = None
        deadline = time.time() + timeout
        try:
            while time.time() < deadline:
                try:
                    msg = json.loads(ws.recv())
                except WebSocketTimeoutException:
                    continue
                except WebSocketConnectionClosedException:
                    break
                if msg["type"] == "error":
                    error_message = msg["message"]
                    break
        finally:
            ws.close()

        assert error_message is not None
        assert "Public key authentication was rejected" in error_message
