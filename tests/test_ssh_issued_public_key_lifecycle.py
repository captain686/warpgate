"""E2E tests for issued SSH public key lifecycle enforcement during auth."""

import time
from pathlib import Path
from uuid import uuid4

import paramiko
import pytest

from .api_client import admin_client, sdk
from .conftest import ProcessManager, WarpgateProcess
from .util import ssh_exec_command_with_public_key, wait_port


def _setup_user_target_and_issue_key(
    processes: ProcessManager,
    shared_wg: WarpgateProcess,
    wg_c_ed25519_pubkey: Path,
    *,
    valid_for_seconds: int | None = None,
    max_uses: int | None = None,
    target_id: str | None = None,
) -> tuple[object, object, str]:
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
        api.add_user_role(user.id, role.id)
        target = api.create_target(
            sdk.TargetDataRequest(
                name=f"ssh-{uuid4()}",
                options=sdk.TargetOptions(
                    sdk.TargetOptionsTargetSSHOptions(
                        kind="Ssh",
                        host="localhost",
                        port=ssh_port,
                        username=processes.ssh_target_username,
                        auth=sdk.SSHTargetAuth(
                            sdk.SSHTargetAuthSshTargetPublicKeyAuth(kind="PublicKey")
                        ),
                    )
                ),
            )
        )
        api.add_target_role(target.id, role.id)
        issued = api.issue_public_key_credential(
            user.id,
            {
                "label": "issued-auth-key",
                "algorithm": "ed25519",
                "valid_for_seconds": valid_for_seconds,
                "max_uses": max_uses,
                "target_id": target_id,
            },
        )

    return user, target, issued.private_key_openssh


def _write_private_key(tmp_path: Path, key_material: str) -> Path:
    key_path = tmp_path / "issued_id_ed25519"
    key_path.write_text(key_material)
    key_path.chmod(0o600)
    return key_path


class TestIssuedPublicKeyLifecycleAuth:
    def test_issued_key_target_scope_is_enforced(
        self,
        processes: ProcessManager,
        wg_c_ed25519_pubkey: Path,
        shared_wg: WarpgateProcess,
        timeout,
        tmp_path: Path,
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
            api.add_user_role(user.id, role.id)
            target_a = api.create_target(
                sdk.TargetDataRequest(
                    name=f"ssh-a-{uuid4()}",
                    options=sdk.TargetOptions(
                        sdk.TargetOptionsTargetSSHOptions(
                            kind="Ssh",
                            host="localhost",
                            port=ssh_port,
                            username=processes.ssh_target_username,
                            auth=sdk.SSHTargetAuth(
                                sdk.SSHTargetAuthSshTargetPublicKeyAuth(kind="PublicKey")
                            ),
                        )
                    ),
                )
            )
            target_b = api.create_target(
                sdk.TargetDataRequest(
                    name=f"ssh-b-{uuid4()}",
                    options=sdk.TargetOptions(
                        sdk.TargetOptionsTargetSSHOptions(
                            kind="Ssh",
                            host="localhost",
                            port=ssh_port,
                            username=processes.ssh_target_username,
                            auth=sdk.SSHTargetAuth(
                                sdk.SSHTargetAuthSshTargetPublicKeyAuth(kind="PublicKey")
                            ),
                        )
                    ),
                )
            )
            api.add_target_role(target_a.id, role.id)
            api.add_target_role(target_b.id, role.id)
            issued = api.issue_public_key_credential(
                user.id,
                {
                    "label": "issued-scoped-key",
                    "algorithm": "ed25519",
                    "target_id": target_a.id,
                },
            )

        key_path = _write_private_key(tmp_path, issued.private_key_openssh)

        status, stdout, _stderr = ssh_exec_command_with_public_key(
            "localhost",
            shared_wg.ssh_port,
            f"{user.username}:{target_a.name}",
            key_path,
            "ls /bin/sh",
            timeout=float(timeout),
        )
        assert status == 0
        assert stdout == b"/bin/sh\n"

        with pytest.raises(paramiko.AuthenticationException):
            ssh_exec_command_with_public_key(
                "localhost",
                shared_wg.ssh_port,
                f"{user.username}:{target_b.name}",
                key_path,
                "ls /bin/sh",
                timeout=float(timeout),
            )

    def test_issued_key_respects_max_uses(
        self,
        processes: ProcessManager,
        wg_c_ed25519_pubkey: Path,
        shared_wg: WarpgateProcess,
        timeout,
        tmp_path: Path,
    ):
        user, target, private_key = _setup_user_target_and_issue_key(
            processes,
            shared_wg,
            wg_c_ed25519_pubkey,
            max_uses=1,
        )
        key_path = _write_private_key(tmp_path, private_key)

        status, stdout, _stderr = ssh_exec_command_with_public_key(
            "localhost",
            shared_wg.ssh_port,
            f"{user.username}:{target.name}",
            key_path,
            "ls /bin/sh",
            timeout=float(timeout),
        )
        assert status == 0
        assert stdout == b"/bin/sh\n"

        with pytest.raises(paramiko.AuthenticationException):
            ssh_exec_command_with_public_key(
                "localhost",
                shared_wg.ssh_port,
                f"{user.username}:{target.name}",
                key_path,
                "ls /bin/sh",
                timeout=float(timeout),
            )

    def test_issued_key_revocation_is_enforced(
        self,
        processes: ProcessManager,
        wg_c_ed25519_pubkey: Path,
        shared_wg: WarpgateProcess,
        timeout,
        tmp_path: Path,
    ):
        user, target, private_key = _setup_user_target_and_issue_key(
            processes,
            shared_wg,
            wg_c_ed25519_pubkey,
            max_uses=5,
        )
        key_path = _write_private_key(tmp_path, private_key)

        url = f"https://localhost:{shared_wg.http_port}"
        with admin_client(url) as api:
            keys = api.get_public_key_credentials(user.id)
            credential = next(k for k in keys if k.label == "issued-auth-key")
            api.revoke_public_key_credential(user.id, credential.id)

        with pytest.raises(paramiko.AuthenticationException):
            ssh_exec_command_with_public_key(
                "localhost",
                shared_wg.ssh_port,
                f"{user.username}:{target.name}",
                key_path,
                "ls /bin/sh",
                timeout=float(timeout),
            )

    def test_issued_key_expiry_is_enforced(
        self,
        processes: ProcessManager,
        wg_c_ed25519_pubkey: Path,
        shared_wg: WarpgateProcess,
        timeout,
        tmp_path: Path,
    ):
        user, target, private_key = _setup_user_target_and_issue_key(
            processes,
            shared_wg,
            wg_c_ed25519_pubkey,
            valid_for_seconds=1,
        )
        key_path = _write_private_key(tmp_path, private_key)

        time.sleep(2)

        with pytest.raises(paramiko.AuthenticationException):
            ssh_exec_command_with_public_key(
                "localhost",
                shared_wg.ssh_port,
                f"{user.username}:{target.name}",
                key_path,
                "ls /bin/sh",
                timeout=float(timeout),
            )
