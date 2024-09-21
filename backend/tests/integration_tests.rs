// use actix_web::{test, web, App};
// use serde_json::json;
// use std::sync::Arc;
// use super::*;
// #[actix_web::test]
// async fn test_get_time() {
//     let db = Arc::new(sled::Config::new().temporary(true).open().unwrap());
//
//     let app = App::new()
//         .app_data(web::Data::new(db.clone()))
//         .route("/api/time", web::get().to(get_time));
//
//     let mut app = test::init_service(app).await;
//
//     let req = test::TestRequest::get().uri("/api/time").to_request();
//     let resp = test::call_service(&mut app, req).await;
//
//     assert!(resp.status().is_success());
//
//     let body = test::read_body(resp).await;
//     let response: TimeResponse = serde_json::from_slice(&body).unwrap();
//     assert!(!response.current_time.is_empty());
// }
//
// #[actix_web::test]
// async fn test_create_session() {
//     let db = Arc::new(sled::Config::new().temporary(true).open().unwrap());
//
//     let app = App::new()
//         .app_data(web::Data::new(db.clone()))
//         .route("/api/sessions", web::post().to(create_session));
//
//     let mut app = test::init_service(app).await;
//
//     // Valid session
//     let payload = json!({
//         "date": "2023-10-01",
//         "session_type": "1-hour"
//     });
//
//     let req = test::TestRequest::post()
//         .uri("/api/sessions")
//         .set_json(&payload)
//         .to_request();
//
//     let resp = test::call_service(&mut app, req).await;
//
//     assert_eq!(resp.status(), actix_web::http::StatusCode::CREATED);
//
//     // Invalid date format
//     let invalid_payload = json!({
//         "date": "2023-13-01", // Invalid month
//         "session_type": "1-hour"
//     });
//
//     let req = test::TestRequest::post()
//         .uri("/api/sessions")
//         .set_json(&invalid_payload)
//         .to_request();
//
//     let resp = test::call_service(&mut app, req).await;
//
//     assert_eq!(resp.status(), actix_web::http::StatusCode::BAD_REQUEST);
//
//     // Invalid session type
//     let invalid_payload = json!({
//         "date": "2023-10-01",
//         "session_type": "4-hours"
//     });
//
//     let req = test::TestRequest::post()
//         .uri("/api/sessions")
//         .set_json(&invalid_payload)
//         .to_request();
//
//     let resp = test::call_service(&mut app, req).await;
//
//     assert_eq!(resp.status(), actix_web::http::StatusCode::BAD_REQUEST);
// }
