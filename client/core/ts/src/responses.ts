import * as Types from "./types.js";

export type ReadResponses = {
  GetVersion: Types.GetVersionResponse;
  GetCoreInfo: Types.GetCoreInfoResponse;
  ListGitProvidersFromConfig: Types.ListGitProvidersFromConfigResponse;
  ListImageRegistriesFromConfig: Types.ListImageRegistriesFromConfigResponse;

  // ==== SWARM ====

  // ==== SERVER ====
  GetServersSummary: Types.GetServersSummaryResponse;
  GetServer: Types.GetServerResponse;
  GetServerState: Types.GetServerStateResponse;
  GetPeripheryInformation: Types.GetPeripheryInformationResponse;
  GetServerActionState: Types.GetServerActionStateResponse;
  ListServers: Types.ListServersResponse;
  ListFullServers: Types.ListFullServersResponse;

  // ==== TERMINAL ====

  // ==== DOCKER ====
  GetContainersSummary: Types.GetContainersSummaryResponse;
  ListAllContainers: Types.ListAllContainersResponse;
  ListContainers: Types.ListContainersResponse;
  GetResourceMatchingContainer: Types.GetResourceMatchingContainerResponse;
  GetContainerLog: Types.GetContainerLogResponse;
  SearchContainerLog: Types.SearchContainerLogResponse;

  // ==== SERVER STATS ====
  GetSystemInformation: Types.GetSystemInformationResponse;
  GetSystemStats: Types.GetSystemStatsResponse;
  GetHistoricalServerStats: Types.GetHistoricalServerStatsResponse;
  ListSystemProcesses: Types.ListSystemProcessesResponse;

  // ==== STACK ====
  GetStacksSummary: Types.GetStacksSummaryResponse;
  GetStack: Types.GetStackResponse;
  GetStackActionState: Types.GetStackActionStateResponse;
  GetStackLog: Types.GetStackLogResponse;
  SearchStackLog: Types.SearchStackLogResponse;
  ListStacks: Types.ListStacksResponse;
  ListFullStacks: Types.ListFullStacksResponse;
  ListStackServices: Types.ListStackServicesResponse;
  ListAllStackServices: Types.ListAllStackServicesResponse;

  // ==== DEPLOYMENT ====

  // ==== BUILD ====

  // ==== REPO ====
  GetReposSummary: Types.GetReposSummaryResponse;
  GetRepo: Types.GetRepoResponse;
  GetRepoActionState: Types.GetRepoActionStateResponse;
  ListRepos: Types.ListReposResponse;
  ListFullRepos: Types.ListFullReposResponse;

  // ==== PROCEDURE ====
  GetProceduresSummary: Types.GetProceduresSummaryResponse;
  GetProcedure: Types.GetProcedureResponse;
  GetProcedureActionState: Types.GetProcedureActionStateResponse;
  ListProcedures: Types.ListProceduresResponse;
  ListFullProcedures: Types.ListFullProceduresResponse;

  // ==== ACTION ====
  GetActionsSummary: Types.GetActionsSummaryResponse;
  GetAction: Types.GetActionResponse;
  GetActionActionState: Types.GetActionActionStateResponse;
  ListActions: Types.ListActionsResponse;
  ListFullActions: Types.ListFullActionsResponse;

  // ==== SCHEDULE ====
  ListSchedules: Types.ListSchedulesResponse;

  // ==== SYNC ====
  GetResourceSyncsSummary: Types.GetResourceSyncsSummaryResponse;
  GetResourceSync: Types.GetResourceSyncResponse;
  GetResourceSyncActionState: Types.GetResourceSyncActionStateResponse;
  ListResourceSyncs: Types.ListResourceSyncsResponse;
  ListFullResourceSyncs: Types.ListFullResourceSyncsResponse;

  // ==== BUILDER ====

  // ==== ALERTER ====

  // ==== TOML ====
  ExportAllResourcesToToml: Types.ExportAllResourcesToTomlResponse;
  ExportResourcesToToml: Types.ExportResourcesToTomlResponse;

  // ==== TAG ====
  GetTag: Types.GetTagResponse;
  ListTags: Types.ListTagsResponse;

  // ==== USER ====
  GetUsername: Types.GetUsernameResponse;
  GetPermission: Types.GetPermissionResponse;
  FindUser: Types.FindUserResponse;
  ListUsers: Types.ListUsersResponse;
  ListApiKeys: Types.ListApiKeysResponse;
  ListApiKeysForServiceUser: Types.ListApiKeysForServiceUserResponse;
  ListPermissions: Types.ListPermissionsResponse;
  ListUserTargetPermissions: Types.ListUserTargetPermissionsResponse;

  // ==== USER GROUP ====
  GetUserGroup: Types.GetUserGroupResponse;
  ListUserGroups: Types.ListUserGroupsResponse;

  // ==== UPDATE ====
  GetUpdate: Types.GetUpdateResponse;
  ListUpdates: Types.ListUpdatesResponse;

  // ==== ALERT ====
  ListAlerts: Types.ListAlertsResponse;
  GetAlert: Types.GetAlertResponse;

  // ==== PROVIDER ====
  GetGitProviderAccount: Types.GetGitProviderAccountResponse;
  ListGitProviderAccounts: Types.ListGitProviderAccountsResponse;
  GetImageRegistryAccount: Types.GetImageRegistryAccountResponse;
  ListImageRegistryAccounts: Types.ListImageRegistryAccountsResponse;

  // ==== ONBOARDING KEY ====
  ListOnboardingKeys: Types.ListOnboardingKeysResponse;
};

export type WriteResponses = {
  // ==== RESOURCE ====
  UpdateResourceMeta: Types.UpdateResourceMetaResponse;

  // ==== SWARM ====

  // ==== SERVER ====
  CreateServer: Types.Server;
  CopyServer: Types.Server;
  DeleteServer: Types.Server;
  UpdateServer: Types.Server;
  RenameServer: Types.Update;
  UpdateServerPublicKey: Types.Update;
  RotateServerKeys: Types.Update;

  // ==== TERMINAL ====

  // ==== STACK ====
  CreateStack: Types.Stack;
  CopyStack: Types.Stack;
  DeleteStack: Types.Stack;
  UpdateStack: Types.Stack;
  RenameStack: Types.Update;
  RefreshStackCache: Types.NoData;

  // ==== DEPLOYMENT ====

  // ==== BUILD ====

  // ==== REPO ====
  CreateRepo: Types.Repo;
  CopyRepo: Types.Repo;
  DeleteRepo: Types.Repo;
  UpdateRepo: Types.Repo;
  RenameRepo: Types.Update;
  RefreshRepoCache: Types.NoData;

  // ==== PROCEDURE ====
  CreateProcedure: Types.Procedure;
  CopyProcedure: Types.Procedure;
  DeleteProcedure: Types.Procedure;
  UpdateProcedure: Types.Procedure;
  RenameProcedure: Types.Update;

  // ==== ACTION ====
  CreateAction: Types.Action;
  CopyAction: Types.Action;
  DeleteAction: Types.Action;
  UpdateAction: Types.Action;
  RenameAction: Types.Update;

  // ==== SYNC ====
  CreateResourceSync: Types.ResourceSync;
  CopyResourceSync: Types.ResourceSync;
  DeleteResourceSync: Types.ResourceSync;
  UpdateResourceSync: Types.ResourceSync;
  RenameResourceSync: Types.Update;
  CommitSync: Types.Update;
  WriteSyncFileContents: Types.Update;
  RefreshResourceSyncPending: Types.ResourceSync;

  // ==== BUILDER ====

  // ==== ALERTER ====

  // ==== ONBOARDING KEY ====
  CreateOnboardingKey: Types.CreateOnboardingKeyResponse;
  UpdateOnboardingKey: Types.UpdateOnboardingKeyResponse;
  DeleteOnboardingKey: Types.DeleteOnboardingKeyResponse;

  // ==== USER ====
  PushRecentlyViewed: Types.PushRecentlyViewedResponse;
  SetLastSeenUpdate: Types.SetLastSeenUpdateResponse;
  CreateLocalUser: Types.CreateLocalUserResponse;
  DeleteUser: Types.DeleteUserResponse;

  // ==== SERVICE USER ====
  CreateServiceUser: Types.CreateServiceUserResponse;
  UpdateServiceUserDescription: Types.UpdateServiceUserDescriptionResponse;
  CreateApiKeyForServiceUser: Types.CreateApiKeyForServiceUserResponse;
  DeleteApiKeyForServiceUser: Types.DeleteApiKeyForServiceUserResponse;

  // ==== USER GROUP ====
  CreateUserGroup: Types.UserGroup;
  RenameUserGroup: Types.UserGroup;
  DeleteUserGroup: Types.UserGroup;
  AddUserToUserGroup: Types.UserGroup;
  RemoveUserFromUserGroup: Types.UserGroup;
  SetUsersInUserGroup: Types.UserGroup;
  SetEveryoneUserGroup: Types.UserGroup;

  // ==== PERMISSIONS ====
  UpdateUserAdmin: Types.UpdateUserAdminResponse;
  UpdateUserBasePermissions: Types.UpdateUserBasePermissionsResponse;
  UpdatePermissionOnResourceType: Types.UpdatePermissionOnResourceTypeResponse;
  UpdatePermissionOnTarget: Types.UpdatePermissionOnTargetResponse;

  // ==== TAG ====
  CreateTag: Types.Tag;
  DeleteTag: Types.Tag;
  RenameTag: Types.Tag;
  UpdateTagColor: Types.Tag;

  // ==== PROVIDER ====
  CreateGitProviderAccount: Types.CreateGitProviderAccountResponse;
  UpdateGitProviderAccount: Types.UpdateGitProviderAccountResponse;
  DeleteGitProviderAccount: Types.DeleteGitProviderAccountResponse;
  CreateImageRegistryAccount: Types.CreateImageRegistryAccountResponse;
  UpdateImageRegistryAccount: Types.UpdateImageRegistryAccountResponse;
  DeleteImageRegistryAccount: Types.DeleteImageRegistryAccountResponse;

  // ==== ALERT ====
  CloseAlert: Types.NoData;
};

export type ExecuteResponses = {
  // ==== STACK ====
  DeployStack: Types.Update;
  BatchDeployStack: Types.BatchExecutionResponse;
  DeployStackIfChanged: Types.Update;
  BatchDeployStackIfChanged: Types.BatchExecutionResponse;
  PullStack: Types.Update;
  BatchPullStack: Types.BatchExecutionResponse;
  StartStack: Types.Update;
  RestartStack: Types.Update;
  StopStack: Types.Update;
  DestroyStack: Types.Update;
  BatchDestroyStack: Types.BatchExecutionResponse;

  // ==== DEPLOYMENT ====

  // ==== BUILD ====

  // ==== REPO ====
  CloneRepo: Types.Update;
  BatchCloneRepo: Types.BatchExecutionResponse;
  PullRepo: Types.Update;
  BatchPullRepo: Types.BatchExecutionResponse;

  // ==== PROCEDURE ====
  RunProcedure: Types.Update;
  BatchRunProcedure: Types.BatchExecutionResponse;
  CancelProcedure: Types.Update;

  // ==== ACTION ====
  RunAction: Types.Update;
  BatchRunAction: Types.BatchExecutionResponse;
  CancelAction: Types.Update;

  // ==== SYNC ====
  RunSync: Types.Update;

  // ==== ALERTER ====

  // ==== SERVER ====
  PruneImages: Types.Update;
  PruneSystem: Types.Update;

  // ==== SWARM ====

  // ==== MAINTENANCE ====
  ClearRepoCache: Types.Update;
  BackupCoreDatabase: Types.Update;
  RotateAllServerKeys: Types.Update;
  RotateCoreKeys: Types.Update;
};
