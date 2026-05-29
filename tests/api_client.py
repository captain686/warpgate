from contextlib import contextmanager

try:
    # in-IDE
    import api_sdk.openapi_client as sdk
except ImportError:
    import openapi_client as sdk


def _unwrap_created_user(response):
    return getattr(response, "user", response)


_original_create_user = sdk.DefaultApi.create_user


def _compat_create_user(self, *args, **kwargs):
    return _unwrap_created_user(_original_create_user(self, *args, **kwargs))


sdk.DefaultApi.create_user = _compat_create_user


@contextmanager
def admin_client(host, token="token-value"):
    config = sdk.Configuration(
        host=f"{host}/@warpgate/admin/api",
        api_key={
            "TokenSecurityScheme": token,
        },
    )
    config.verify_ssl = False
    with sdk.ApiClient(config) as api_client:
        yield sdk.DefaultApi(api_client)
