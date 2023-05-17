use crate::api::{ImageRef, ImageState, PodRef};
use crate::store::Store;
use actix_web::{get, web, App, HttpResponse, HttpServer, Responder};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub bind_addr: String,
}

#[get("/v1/images")]
async fn get_containers(store: web::Data<Store<ImageRef, PodRef, ()>>) -> impl Responder {
    HttpResponse::Ok().json(
        store
            .get_state()
            .await
            .into_iter()
            .map(|(k, v)| (k, ImageState { pods: v.owners }))
            .collect::<HashMap<_, _>>(),
    )
}

/*
#[get("/v1/images/{namespace}")]
async fn get_containers_ns(path: web::Path<String>, store: web::Data<Store>) -> impl Responder {
    let ns = path.into_inner();
    HttpResponse::Ok().json(store.get_containers_ns(&ns).await)
}*/

pub async fn run(config: ServerConfig, store: Store<ImageRef, PodRef, ()>) -> anyhow::Result<()> {
    let store = web::Data::new(store);

    HttpServer::new(move || {
        App::new().app_data(store.clone()).service(get_containers)
        //.service(get_containers_ns)
    })
    .bind(&config.bind_addr)?
    .run()
    .await?;

    Ok(())
}
