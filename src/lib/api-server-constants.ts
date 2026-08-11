import { isBrowserRuntime, serveSessionToken } from "./platform"

export const API_SERVER_PORT = 19828
// In serve mode the UI is same-origin with the API and the port may have
// been probed past the default, so reflect the actual origin instead of
// the constant.
export const API_SERVER_BASE_URL =
  isBrowserRuntime && serveSessionToken ? window.location.origin : `http://127.0.0.1:${API_SERVER_PORT}`
export const API_SERVER_HEALTH_URL = `${API_SERVER_BASE_URL}/api/v1/health`
