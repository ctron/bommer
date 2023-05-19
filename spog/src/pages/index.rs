use crate::backend;
use crate::backend::WorkloadService;
use crate::components::{remote_content, workload::WorkloadTable};
use crate::hooks::use_backend;
use patternfly_yew::prelude::*;
use std::rc::Rc;
use yew::prelude::*;
use yew_more_hooks::prelude::use_async_with_cloned_deps;

#[function_component(Index)]
pub fn index() -> Html {
    let backend = use_backend();

    let fetch = use_async_with_cloned_deps(
        |backend| async move {
            Ok::<_, backend::Error>(Rc::new(
                WorkloadService::new((*backend).clone()).lookup().await?,
            ))
        },
        backend,
    );

    html!(
        <>
            <PageSection
                variant={PageSectionVariant::Light}
                shadow={PageSectionShadow::Bottom}
                fill=false
            >
                <Content>
                    <Title level={Level::H1}>{"Discovered Workload"}</Title>
                </Content>
            </PageSection>

            <PageSection variant={PageSectionVariant::Default} fill=true>
            {
                remote_content(&fetch, |workload|{
                    html!(
                        <WorkloadTable workload={workload} />
                    )
                })
            }
            </PageSection>

        </>
    )
}
