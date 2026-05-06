import { Container, getContainer } from "@cloudflare/containers";
import { OPENAPI_SCHEMA } from "./openapi-schema";

const OPENAPI_PATH = "/openapi.json";
const DOCS_PATH = "/docs";
const REGISTER_PATH = "/v1/sessions/register";
const SESSION_PATH_PATTERN = /^\/v1\/sessions\/([^/]+)/;

export class VerifierContainer extends Container<Env> {
  defaultPort = 7047;
  sleepAfter = "15m";
  envVars: Record<string, string> = {
    NOTARY_SIGNING_KEY_B64: this.env.NOTARY_SIGNING_KEY_B64 ?? "",
  };
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);

    if (url.pathname === OPENAPI_PATH) {
      return openApiResponse(request);
    }

    if (url.pathname === DOCS_PATH || url.pathname === `${DOCS_PATH}/`) {
      return docsResponse(request);
    }

    if (url.pathname === REGISTER_PATH) {
      return routeRegister(request, env, url);
    }

    const sessionId = sessionIdFromPath(url.pathname);
    const instanceName = sessionId ?? "control";
    return getContainer(env.VERIFIER_CONTAINER, instanceName).fetch(request);
  },
};

function openApiResponse(request: Request): Response {
  if (request.method !== "GET" && request.method !== "HEAD") {
    return jsonError(405, "method_not_allowed", "OpenAPI schema requires GET");
  }

  return new Response(
    request.method === "HEAD" ? null : JSON.stringify(OPENAPI_SCHEMA, null, 2),
    {
      headers: {
        "access-control-allow-origin": "*",
        "content-type": "application/vnd.oai.openapi+json; charset=utf-8",
      },
    },
  );
}

function docsResponse(request: Request): Response {
  if (request.method !== "GET" && request.method !== "HEAD") {
    return jsonError(405, "method_not_allowed", "API docs require GET");
  }

  return new Response(request.method === "HEAD" ? null : docsHtml(), {
    headers: {
      "content-security-policy":
        "default-src 'none'; img-src data: https://validator.swagger.io; style-src 'unsafe-inline' https://unpkg.com; script-src 'unsafe-inline' https://unpkg.com; connect-src 'self'; font-src https://unpkg.com",
      "content-type": "text/html; charset=utf-8",
      "x-content-type-options": "nosniff",
    },
  });
}

function docsHtml(): string {
  return `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>MPC Verifier API</title>
    <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css">
    <style>
      body { margin: 0; background: #f7f7f7; }
      .swagger-ui .topbar { display: none; }
    </style>
  </head>
  <body>
    <div id="swagger-ui"></div>
    <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
    <script>
      window.ui = SwaggerUIBundle({
        dom_id: "#swagger-ui",
        url: "${OPENAPI_PATH}",
        deepLinking: true,
        displayOperationId: true,
        persistAuthorization: false
      });
    </script>
  </body>
</html>`;
}

async function routeRegister(request: Request, env: Env, url: URL): Promise<Response> {
  if (request.method !== "POST") {
    return jsonError(405, "method_not_allowed", "registration requires POST");
  }

  let body: Record<string, unknown>;
  try {
    const parsed = await request.json();
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return jsonError(400, "invalid_request", "registration body must be a JSON object");
    }
    body = parsed as Record<string, unknown>;
  } catch {
    return jsonError(400, "invalid_json", "registration body must be valid JSON");
  }

  const sessionId = typeof body.sessionId === "string" && body.sessionId.trim().length > 0
    ? body.sessionId.trim()
    : crypto.randomUUID();

  const headers = new Headers(request.headers);
  headers.set("content-type", "application/json");
  headers.delete("content-length");

  const routedRequest = new Request(url.toString(), {
    method: request.method,
    headers,
    body: JSON.stringify({ ...body, sessionId }),
  });

  return getContainer(env.VERIFIER_CONTAINER, sessionId).fetch(routedRequest);
}

function sessionIdFromPath(pathname: string): string | undefined {
  const match = pathname.match(SESSION_PATH_PATTERN);
  return match?.[1];
}

function jsonError(status: number, code: string, message: string): Response {
  return Response.json(
    {
      code,
      message,
    },
    { status },
  );
}
