"""Tests for global log retention parameter controls."""

from .api_client import admin_client, sdk
from .conftest import WarpgateProcess


def test_update_log_retention_parameters(shared_wg: WarpgateProcess):
    """Admin API should persist log retention strategy and max size settings."""
    url = f"https://localhost:{shared_wg.http_port}"

    with admin_client(url) as api:
        original = api.get_parameters()

        try:
            api.update_parameters(
                sdk.ParameterUpdate(
                    allow_own_credential_management=original.allow_own_credential_management,
                    log_retention_strategy=sdk.LogRetentionStrategy.MAX_SIZE,
                    log_max_size_megabytes=256,
                )
            )

            updated = api.get_parameters()
            assert updated.log_retention_strategy == sdk.LogRetentionStrategy.MAX_SIZE
            assert updated.log_max_size_megabytes == 256
        finally:
            api.update_parameters(
                sdk.ParameterUpdate(
                    allow_own_credential_management=original.allow_own_credential_management,
                    log_retention_strategy=original.log_retention_strategy,
                    log_max_size_megabytes=original.log_max_size_megabytes,
                )
            )
