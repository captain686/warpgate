use poem_openapi::OpenApiService;
use regex::Regex;
use warpgate_common::version::warpgate_version;
use warpgate_protocol_http::api;

pub fn main() -> Result<(), regex::Error> {
    let api_service = OpenApiService::new(api::get(), "Warpgate HTTP proxy", warpgate_version())
        .server("/@warpgate/api");

    let spec = api_service.spec();
    let re = Regex::new(r"PaginatedResponse<(?P<name>\w+)>")?;
    let spec = re.replace_all(&spec, "Paginated$name");

    println!("{spec}");
    Ok(())
}
