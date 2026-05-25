"""Regression tests for the admin logs API."""

from .api_client import sdk, admin_client
from .conftest import WarpgateProcess


def test_get_logs_allows_immediate_repeat_requests(shared_wg: WarpgateProcess):
    """Identical sequential /logs requests should not be rejected as too frequent."""
    url = f"https://localhost:{shared_wg.http_port}"
    request = sdk.GetLogsRequest(search="", limit=10)

    with admin_client(url) as api:
        first = api.get_logs_with_http_info(request)
        second = api.get_logs_with_http_info(request)

    assert first.status_code == 200
    assert second.status_code == 200
