use utoipa::OpenApi;

use crate::api::read;

#[derive(OpenApi)]
#[openapi(
  paths(
    // misc
    read::get_version,
    read::get_core_info,
    read::list_secrets,
    read::list_onboarding_keys,
    read::list_schedules,
    // git / registry providers
    read::get_git_provider_account,
    read::list_git_provider_accounts,
    read::list_git_providers_from_config,
    read::get_image_registry_account,
    read::list_image_registry_accounts,
    read::list_image_registries_from_config,
    // swarm
    // server
    read::list_servers,
    read::list_full_servers,
    read::get_server,
    read::get_server_action_state,
    read::get_servers_summary,
    read::get_server_state,
    read::get_periphery_information,
    read::get_system_information,
    read::get_system_stats,
    read::list_system_processes,
    read::get_historical_server_stats,
    // stack
    read::list_stacks,
    read::list_full_stacks,
    read::get_stack,
    read::get_stack_action_state,
    read::get_stacks_summary,
    read::list_stack_services,
    read::list_all_stack_services,
    read::get_stack_log,
    read::search_stack_log,
    // deployment
    // build
    // repo
    read::list_repos,
    read::list_full_repos,
    read::get_repo,
    read::get_repo_action_state,
    read::get_repos_summary,
    // procedure
    read::list_procedures,
    read::list_full_procedures,
    read::get_procedure,
    read::get_procedure_action_state,
    read::get_procedures_summary,
    // action
    read::list_actions,
    read::list_full_actions,
    read::get_action,
    read::get_action_action_state,
    read::get_actions_summary,
    // builder
    // alerter
    // resource_sync
    read::list_resource_syncs,
    read::list_full_resource_syncs,
    read::get_resource_sync,
    read::get_resource_sync_action_state,
    read::get_resource_syncs_summary,
    // variable
    read::list_variables,
    read::get_variable,
    // container
    read::get_containers_summary,
    read::list_all_containers,
    read::list_containers,
    read::get_resource_matching_container,
    read::get_container_log,
    read::search_container_log,
    // toml
    read::export_all_resources_to_toml,
    read::export_resources_to_toml,
    // terminal
    // tag
    read::list_tags,
    read::get_tag,
    // update
    read::list_updates,
    read::get_update,
    // alert
    read::list_alerts,
    read::get_alert,
    // user
    read::list_api_keys,
    read::list_api_keys_for_service_user,
    read::list_users,
    read::find_user,
    read::get_username,
    // permissions
    read::list_permissions,
    read::get_permission,
    read::list_user_target_permissions,
    // user group
    read::list_user_groups,
    read::get_user_group,
  ),
)]
pub struct KomodoReadApi;
