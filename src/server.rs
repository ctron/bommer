use crate::bombastic::Map;
use actix_web::{get, web, App, HttpResponse, HttpServer, Responder};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub bind_addr: String,
}

#[get("/v1/images")]
async fn get_containers(map: web::Data<Map>) -> impl Responder {
    HttpResponse::Ok().json(map.get_state().await.into_iter().collect::<HashMap<_, _>>())
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
        App::new().app_data(map.clone()).service(get_containers)
        //.service(get_containers_ns)
    })
    .bind(&config.bind_addr)?
    .run()
    .await?;

    Ok(())
}
