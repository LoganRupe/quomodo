use utoipa::OpenApi;

use crate::api::write;

#[derive(OpenApi)]
#[openapi(
  paths(
    // onboarding key
    write::create_onboarding_key,
    write::update_onboarding_key,
    write::delete_onboarding_key,
    // resource
    write::update_resource_meta,
    // swarm
    // server
    write::create_server,
    write::copy_server,
    write::delete_server,
    write::update_server,
    write::rename_server,
    write::update_server_public_key,
    write::rotate_server_keys,
    // stack
    write::create_stack,
    write::copy_stack,
    write::delete_stack,
    write::update_stack,
    write::rename_stack,
    write::refresh_stack_cache,
    // deployment
    // build
    // repo
    write::create_repo,
    write::copy_repo,
    write::delete_repo,
    write::update_repo,
    write::rename_repo,
    write::refresh_repo_cache,
    // procedure
    write::create_procedure,
    write::copy_procedure,
    write::delete_procedure,
    write::update_procedure,
    write::rename_procedure,
    // action
    write::create_action,
    write::copy_action,
    write::delete_action,
    write::update_action,
    write::rename_action,
    // builder
    // alerter
    // resource_sync
    write::create_resource_sync,
    write::copy_resource_sync,
    write::delete_resource_sync,
    write::update_resource_sync,
    write::rename_resource_sync,
    write::refresh_resource_sync_pending,
    write::write_sync_file_contents,
    write::commit_sync,
    // terminal
    // alert
    write::close_alert,
    // tags
    write::create_tag,
    write::delete_tag,
    write::rename_tag,
    write::update_tag_color,
    // permissions
    write::update_permission_on_target,
    write::update_permission_on_resource_type,
    write::update_user_base_permissions,
    write::update_user_admin,
    // user group
    write::create_user_group,
    write::rename_user_group,
    write::delete_user_group,
    write::add_user_to_user_group,
    write::remove_user_from_user_group,
    write::set_users_in_user_group,
    write::set_everyone_user_group,
    // user
    write::push_recently_viewed,
    write::set_last_seen_update,
    write::delete_user,
    write::create_local_user,
    write::create_service_user,
    write::update_service_user_description,
    // api key
    write::create_api_key_for_service_user,
    write::delete_api_key_for_service_user,
    // provider
    write::create_git_provider_account,
    write::update_git_provider_account,
    write::delete_git_provider_account,
    write::create_image_registry_account,
    write::update_image_registry_account,
    write::delete_image_registry_account,
  ),
)]
pub struct KomodoWriteApi;
