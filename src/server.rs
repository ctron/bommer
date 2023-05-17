use crate::store::Store;
use actix_web::{get, web, App, HttpResponse, HttpServer, Responder};

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub bind_addr: String,
}

#[get("/v1/images")]
async fn get_containers(store: web::Data<Store>) -> impl Responder {
    HttpResponse::Ok().json(store.get_containers_all().await)
}

/*
#[get("/v1/images/{namespace}")]
async fn get_containers_ns(path: web::Path<String>, store: web::Data<Store>) -> impl Responder {
    let ns = path.into_inner();
    HttpResponse::Ok().json(store.get_containers_ns(&ns).await)
}*/

pub async fn run(config: ServerConfig, store: Store) -> anyhow::Result<()> {
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
