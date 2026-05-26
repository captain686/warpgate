"""API-level tests for issued SSH public key credential lifecycle."""

from datetime import datetime, timedelta, timezone
from uuid import uuid4

from .api_client import admin_client, sdk
from .conftest import WarpgateProcess


def test_issue_public_key_credential_sets_lifecycle_fields(shared_wg: WarpgateProcess):
    """Issuing a key should set issued/lifecycle metadata and return private key once."""
    url = f"https://localhost:{shared_wg.http_port}"
    with admin_client(url) as api:
        user = api.create_user(sdk.CreateUserRequest(username=f"user-{uuid4()}"))
        before_issue = datetime.now(timezone.utc)
        issued = api.issue_public_key_credential(
            user.id,
            sdk.IssuePublicKeyCredentialRequest(
                label="issued-key",
                algorithm=sdk.IssuedPublicKeyAlgorithm.ED25519,
                valid_for_seconds=120,
                max_uses=3,
            ),
        )

        credential = issued.credential
        assert credential.issued_by_warpgate is True
        assert credential.max_uses == 3
        assert credential.uses_left == 3
        assert credential.revoked_at is None
        assert credential.expires_at is not None
        assert "PRIVATE KEY" in issued.private_key_openssh

        # Expires-at should be close to now + valid_for_seconds
        expires_at = credential.expires_at
        assert expires_at is not None
        if expires_at.tzinfo is None:
            expires_at = expires_at.replace(tzinfo=timezone.utc)
        lower_bound = before_issue + timedelta(seconds=100)
        upper_bound = before_issue + timedelta(seconds=140)
        assert lower_bound <= expires_at <= upper_bound


def test_issue_public_key_credential_without_limits_is_unbounded(shared_wg: WarpgateProcess):
    """Issuing without validity/uses should keep expiry and usage counters unset."""
    url = f"https://localhost:{shared_wg.http_port}"
    with admin_client(url) as api:
        user = api.create_user(sdk.CreateUserRequest(username=f"user-{uuid4()}"))
        issued = api.issue_public_key_credential(
            user.id,
            sdk.IssuePublicKeyCredentialRequest(
                label="issued-key-unbounded",
                algorithm=sdk.IssuedPublicKeyAlgorithm.ED25519,
            ),
        )

        credential = issued.credential
        assert credential.issued_by_warpgate is True
        assert credential.expires_at is None
        assert credential.max_uses is None
        assert credential.uses_left is None


def test_revoke_issued_public_key_sets_revocation_and_zero_uses(shared_wg: WarpgateProcess):
    """Revoking an issued key should mark it revoked and set uses_left to zero."""
    url = f"https://localhost:{shared_wg.http_port}"
    with admin_client(url) as api:
        user = api.create_user(sdk.CreateUserRequest(username=f"user-{uuid4()}"))
        issued = api.issue_public_key_credential(
            user.id,
            sdk.IssuePublicKeyCredentialRequest(
                label="issued-key-revoke",
                algorithm=sdk.IssuedPublicKeyAlgorithm.ED25519,
                valid_for_seconds=300,
                max_uses=5,
            ),
        )
        credential_id = issued.credential.id

        api.revoke_public_key_credential(user.id, credential_id)

        keys = api.get_public_key_credentials(user.id)
        revoked = next(k for k in keys if k.id == credential_id)
        assert revoked.revoked_at is not None
        assert revoked.uses_left == 0
