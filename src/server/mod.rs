mod ws;

use crate::bombastic::Map;
use actix_cors::Cors;
use actix_web::{get, web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use std::collections::HashMap;
use tokio::task::spawn_local;

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub bind_addr: String,
}

#[get("/api/v1/workload")]
async fn get_workload(map: web::Data<Map>) -> impl Responder {
    HttpResponse::Ok().json(map.get_state().await.into_iter().collect::<HashMap<_, _>>())
}

#[get("/api/v1/workload_stream")]
pub async fn workload_stream(
    req: HttpRequest,
    stream: web::Payload,
    map: web::Data<Map>,
) -> Result<HttpResponse, actix_web::Error> {
    let (res, session, msg_stream) = actix_ws::handle(&req, stream)?;
    let subscription = map.subscribe(32).await;
    spawn_local(ws::run(subscription, session, msg_stream));
    Ok(res)
}

/*
#[get("/v1/images/{namespace}")]
async fn get_containers_ns(path: web::Path<String>, store: web::Data<Store>) -> impl Responder {
    let ns = path.into_inner();
    HttpResponse::Ok().json(store.get_containers_ns(&ns).await)
}*/

pub async fn run(config: ServerConfig, map: Map) -> anyhow::Result<()> {
    let map = web::Data::new(map);

    HttpServer::new(move || {
        let cors = Cors::default()
            .send_wildcard()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header()
            .max_age(3600);

        App::new()
            .app_data(map.clone())
            .wrap(cors)
            .service(get_workload)
            .service(workload_stream)
        //.service(get_containers_ns)
    })
    .bind(&config.bind_addr)?
    .run()
    .await?;

    Ok(())
}
