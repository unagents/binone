import { callNative } from "~/utils/api/transport";
import { toApiError } from "~/utils/api/errors";
import type { ArgsOf, OperationName, ResultOf } from "~/utils/api/operations";

/**
 * Calls an operation on the server through the native side. Rejects with an `ApiError`
 * (`kind` + `detail`) and never with anything else.
 */
export async function callApi<K extends OperationName>(op: K, ...args: ArgsOf<K> extends undefined ? [] : [ArgsOf<K>]): Promise<ResultOf<K>> {
  try {
    return await callNative<ResultOf<K>>("api_call", { op, args: args[0] ?? null });
  } catch (error) {
    throw toApiError(error);
  }
}

/** The server's operations by name: `const api = useApi(); await api.requests({ filter })`. */
export function useApi() {
  return {
    call: callApi,
    serverInfo: () => callApi("serverInfo"),
    requests: (args?: ArgsOf<"requests">) => (args ? callApi("requests", args) : callApi("requests", {})),
    request: (id: string) => callApi("request", { id }),
    requestDetail: (id: string) => callApi("requestDetail", { id }),
    artifact: (requestId: string, artifactRef: string) => callApi("artifact", { requestId, artifactRef }),
    conversation: (requestId: string) => callApi("conversation", { requestId }),
    notifications: (limit?: number) => callApi("notifications", { limit: limit ?? null }),
    projects: () => callApi("projects"),
    settings: () => callApi("settings"),
    agentMcpConfig: () => callApi("agentMcpConfig"),
    saveDraft: (draft: ArgsOf<"saveDraft">) => callApi("saveDraft", draft),
    submitRequest: (id: string, version: number) => callApi("submitRequest", { id, version }),
    checkSubmitRequest: (id: string, version: number) => callApi("checkSubmitRequest", { id, version }),
    submitRequests: (items: ArgsOf<"submitRequests">["items"]) => callApi("submitRequests", { items }),
    decideApproval: (args: ArgsOf<"decideApproval">) => callApi("decideApproval", args),
    answerQuestion: (args: ArgsOf<"answerQuestion">) => callApi("answerQuestion", args),
    createProject: (name: string) => callApi("createProject", { name }),
    updateSettings: (settings: ArgsOf<"updateSettings">["settings"]) => callApi("updateSettings", { settings }),
    registerConversation: (requestId: string, conversationId: string) =>
      callApi("registerConversation", { requestId, conversationId }),
  };
}
