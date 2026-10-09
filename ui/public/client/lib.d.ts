import { ExecuteResponses, ReadResponses, WriteResponses } from "./responses.js";
import { ExecuteRequest, ReadRequest, Update, UpdateListItem, User, WriteRequest } from "./types.js";
export * as MoghAuth from "npm:mogh_auth_client";
export * as Types from "./types.js";
export type { ExecuteResponses, ReadResponses, WriteResponses, };
export type InitOptions = {
    type: "jwt";
    params: {
        jwt: string;
    };
} | {
    type: "api-key";
    params: {
        key: string;
        secret: string;
    };
};
export declare class CancelToken {
    cancelled: boolean;
    constructor();
    cancel(): void;
}
export type ClientState = {
    jwt: string | undefined;
    key: string | undefined;
    secret: string | undefined;
};
/** Initialize a new client for Komodo */
export declare function KomodoClient(url: string, options: InitOptions): {
    /**
     * Call the `/auth` api.
     *
     * ```
     * const { jwt } = await komodo.auth.login("LoginLocalUser", {
     *   username: "test-user",
     *   password: "test-pass"
     * });
     * ```
     *
     * https://docs.rs/mogh_auth_client/latest/mogh_auth_client/api/index.html
     */
    auth: {
        login: <T extends import("mogh_auth_client/dist/types.js").LoginRequest["type"], Req extends Extract<import("mogh_auth_client/dist/types.js").LoginRequest, {
            type: T;
        }>>(type: T, params: Req["params"]) => Promise<import("mogh_auth_client").LoginResponses[Req["type"]]>;
        manage: <T extends import("mogh_auth_client/dist/types.js").ManageRequest["type"], Req extends Extract<import("mogh_auth_client/dist/types.js").ManageRequest, {
            type: T;
        }>>(type: T, params: Req["params"]) => Promise<import("mogh_auth_client").ManageResponses[Req["type"]]>;
        externalLogin: (provider: import("mogh_auth_client/dist/types.js").ExternalLoginProvider) => void;
    };
    /**
     * Get the current (calling) user.
     *
     * ```
     * const user = await komodo.getUser();
     * ```
     *
     * https://docs.rs/komodo_client/latest/komodo_client/api/user/index.html
     */
    getUser: () => Promise<User>;
    /**
     * Call the `/read` api.
     *
     * ```
     * const stack = await komodo.read("GetStack", {
     *   stack: "my-stack"
     * });
     * ```
     *
     * https://docs.rs/komodo_client/latest/komodo_client/api/read/index.html
     */
    read: <T extends ReadRequest["type"], Req extends Extract<ReadRequest, {
        type: T;
    }>>(type: T, params: Req["params"]) => Promise<ReadResponses[Req["type"]]>;
    /**
     * Call the `/write` api.
     *
     * ```
     * const build = await komodo.write("UpdateBuild", {
     *   id: "my-build",
     *   config: {
     *     version: "1.0.4"
     *   }
     * });
     * ```
     *
     * https://docs.rs/komodo_client/latest/komodo_client/api/write/index.html
     */
    write: <T extends WriteRequest["type"], Req extends Extract<WriteRequest, {
        type: T;
    }>>(type: T, params: Req["params"]) => Promise<WriteResponses[Req["type"]]>;
    /**
     * Call the `/execute` api.
     *
     * ```
     * const update = await komodo.execute("DeployStack", {
     *   stack: "my-stack"
     * });
     * ```
     *
     * NOTE. These calls return immediately when the update is created, NOT when the execution task finishes.
     * To have the call only return when the task finishes, use [execute_and_poll_until_complete].
     *
     * https://docs.rs/komodo_client/latest/komodo_client/api/execute/index.html
     */
    execute: <T extends ExecuteRequest["type"], Req extends Extract<ExecuteRequest, {
        type: T;
    }>>(type: T, params: Req["params"]) => Promise<ExecuteResponses[Req["type"]]>;
    /**
     * Call the `/execute` api, and poll the update until the task has completed.
     *
     * ```
     * const update = await komodo.execute_and_poll("DeployStack", {
     *   stack: "my-stack"
     * });
     * ```
     *
     * https://docs.rs/komodo_client/latest/komodo_client/api/execute/index.html
     */
    execute_and_poll: <T extends ExecuteRequest["type"], Req extends Extract<ExecuteRequest, {
        type: T;
    }>>(type: T, params: Req["params"]) => Promise<(Update | {
        status: "Err";
        data: import("./types.js").BatchExecutionResponseItemErr;
    })[] | Update>;
    /**
     * Poll an Update (returned by the `execute` calls) until the `status` is `Complete`.
     * https://docs.rs/komodo_client/latest/komodo_client/entities/update/struct.Update.html#structfield.status.
     */
    poll_update_until_complete: (update_id: string) => Promise<Update>;
    /** Returns the version of Komodo Core the client is calling to. */
    core_version: () => Promise<string>;
    /**
     * Connects to update websocket, performs login and attaches handlers,
     * and returns the WebSocket handle.
     */
    get_update_websocket: ({ on_update, on_login, on_open, on_close, }: {
        on_update: (update: UpdateListItem) => void;
        on_login?: () => void;
        on_open?: () => void;
        on_close?: () => void;
    }) => WebSocket;
    /**
     * Subscribes to the update websocket with automatic reconnect loop.
     *
     * Note. Awaiting this method will never finish.
     */
    subscribe_to_update_websocket: ({ on_update, on_open, on_login, on_close, retry, retry_timeout_ms, cancel, on_cancel, }: {
        on_update: (update: UpdateListItem) => void;
        on_login?: () => void;
        on_open?: () => void;
        on_close?: () => void;
        retry?: boolean;
        retry_timeout_ms?: number;
        cancel?: CancelToken;
        on_cancel?: () => void;
    }) => Promise<void>;
};
