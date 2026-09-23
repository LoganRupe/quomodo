use utoipa::OpenApi;

use crate::api::execute;

#[derive(OpenApi)]
#[openapi(
  paths(
    // swarm
    // server
    execute::prune_images,
    execute::prune_system,
    // stack
    execute::deploy_stack,
    execute::batch_deploy_stack,
    execute::deploy_stack_if_changed,
    execute::batch_deploy_stack_if_changed,
    execute::pull_stack,
    execute::batch_pull_stack,
    execute::start_stack,
    execute::restart_stack,
    execute::stop_stack,
    execute::destroy_stack,
    execute::batch_destroy_stack,
    // deployment
    // build
    // repo
    execute::clone_repo,
    execute::batch_clone_repo,
    execute::pull_repo,
    execute::batch_pull_repo,
    // procedure
    execute::run_procedure,
    execute::batch_run_procedure,
    execute::cancel_procedure,
    // action
    execute::run_action,
    execute::batch_run_action,
    execute::cancel_action,
    // alerter
    // resource_sync
    execute::run_sync,
    // maintenance
    execute::clear_repo_cache,
    execute::backup_core_database,
    execute::rotate_all_server_keys,
    execute::rotate_core_keys,
  ),
)]
pub struct KomodoExecuteApi;
