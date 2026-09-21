use actix_web::{HttpResponse, web};
use fingest_contracts::CapabilitiesDto;

/// Which plugins this binary was compiled with and which of them `PLUGINS` turned on.
///
/// Built by the composition root and registered as app data. Deliberately not the plugin
/// types themselves: this adapter must not learn what a `Plugin` is, or the HTTP layer
/// would start depending on the extension mechanism it is meant to merely describe.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapabilityReport {
    pub enabled: Vec<String>,
    pub available: Vec<String>,
    pub capabilities: Vec<String>,
}

/// Public on purpose: a client needs this before it has a token, to decide what to render
/// on the login screen. It exposes build configuration, never data.
pub async fn get_capabilities(report: Option<web::Data<CapabilityReport>>) -> HttpResponse {
    // An unregistered report means nothing is enabled rather than a 500, so a wiring slip
    // degrades to a smaller UI instead of a broken one.
    let report = report.map(|r| r.get_ref().clone()).unwrap_or_default();

    HttpResponse::Ok().json(CapabilitiesDto {
        enabled: report.enabled,
        available: report.available,
        capabilities: report.capabilities,
    })
}

pub fn capability_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(web::resource("/api/capabilities").route(web::get().to(get_capabilities)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{App, http::StatusCode, test};

    fn report() -> CapabilityReport {
        CapabilityReport {
            enabled: vec!["tracing".into()],
            available: vec!["tracing".into(), "in-process".into()],
            capabilities: vec!["budget-forecast".into()],
        }
    }

    async fn get(app_data: Option<CapabilityReport>) -> (StatusCode, CapabilitiesDto) {
        let mut app = App::new().configure(capability_routes);
        if let Some(data) = app_data {
            app = app.app_data(web::Data::new(data));
        }
        let service = test::init_service(app).await;
        let res = test::call_service(
            &service,
            test::TestRequest::get()
                .uri("/api/capabilities")
                .to_request(),
        )
        .await;

        let status = res.status();
        (status, test::read_body_json(res).await)
    }

    #[actix_web::test]
    async fn reports_what_the_composition_root_registered() {
        let (status, body) = get(Some(report())).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.enabled, ["tracing"]);
        assert_eq!(body.available, ["tracing", "in-process"]);
        assert_eq!(body.capabilities, ["budget-forecast"]);
    }

    #[actix_web::test]
    async fn needs_no_token() {
        let (status, _) = get(Some(report())).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[actix_web::test]
    async fn an_unregistered_report_reads_as_nothing_enabled() {
        let (status, body) = get(None).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, CapabilitiesDto::default());
    }
}
