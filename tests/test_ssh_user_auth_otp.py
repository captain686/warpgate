from base64 import b64decode
from uuid import uuid4
import paramiko
import pyotp
import pytest
from pathlib import Path

from .api_client import admin_client, sdk
from .conftest import ProcessManager, WarpgateProcess
from .util import ssh_exec_command_with_public_key, wait_port


class Test:
    def test_otp(
        self,
        processes: ProcessManager,
        wg_c_ed25519_pubkey: Path,
        otp_key_base32: str,
        otp_key_base64: str,
        timeout,
        shared_wg: WarpgateProcess,
    ):
        ssh_port = processes.start_ssh_server(
            trusted_keys=[wg_c_ed25519_pubkey.read_text()]
        )

        wait_port(ssh_port)

        url = f"https://localhost:{shared_wg.http_port}"
        with admin_client(url) as api:
            role = api.create_role(
                sdk.RoleDataRequest(name=f"role-{uuid4()}"),
            )
            user = api.create_user(sdk.CreateUserRequest(username=f"user-{uuid4()}"))
            api.create_public_key_credential(
                user.id,
                sdk.NewPublicKeyCredential(
                    label="Public Key",
                    openssh_public_key=open("ssh-keys/id_ed25519.pub").read().strip(),
                ),
            )
            api.create_otp_credential(
                user.id,
                sdk.NewOtpCredential(
                    secret_key=list(b64decode(otp_key_base64)),
                ),
            )
            api.update_user(
                user.id,
                sdk.UserDataRequest(
                    username=user.username,
                    credential_policy=sdk.UserRequireCredentialsPolicy(
                        ssh=["PublicKey", "Totp"],
                    ),
                ),
            )
            api.add_user_role(user.id, role.id)
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

        totp = pyotp.TOTP(otp_key_base32)
        status, stdout, _stderr = ssh_exec_command_with_public_key(
            "localhost",
            shared_wg.ssh_port,
            f"{user.username}:{ssh_target.name}",
            "ssh-keys/id_ed25519",
            "ls /bin/sh",
            timeout=float(timeout),
            otp_code=totp.now(),
        )
        assert status == 0
        assert stdout == b"/bin/sh\n"

        with pytest.raises(paramiko.AuthenticationException):
            ssh_exec_command_with_public_key(
                "localhost",
                shared_wg.ssh_port,
                f"{user.username}:{ssh_target.name}",
                "ssh-keys/id_ed25519",
                "ls /bin/sh",
                timeout=float(timeout),
                otp_code="12345678",
            )
