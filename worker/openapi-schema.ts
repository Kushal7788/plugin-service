export const OPENAPI_SCHEMA = {
  openapi: "3.0.3",
  info: {
    title: "Mobile TLSN Verifier Service API",
    version: "0.1.0",
    description:
      "HTTP and WebSocket API for registering TLSN verifier sessions, issuing attestations, polling verifier results, and verifying exported proof artifacts.",
  },
  servers: [
    {
      url: "/",
    },
  ],
  paths: {
    "/health": {
      get: {
        operationId: "health",
        summary: "Health check",
        responses: {
          "200": {
            description: "Service is running.",
            content: {
              "text/plain": {
                schema: {
                  type: "string",
                  example: "ok",
                },
              },
            },
          },
        },
      },
    },
    "/v1/info": {
      get: {
        operationId: "getInfo",
        summary: "Service info",
        responses: {
          "200": {
            description: "Service metadata.",
            content: {
              "application/json": {
                schema: {
                  $ref: "#/components/schemas/InfoResponse",
                },
              },
            },
          },
        },
      },
    },
    "/v1/sessions/register": {
      post: {
        operationId: "registerSession",
        summary: "Register a verifier session",
        requestBody: {
          required: true,
          content: {
            "application/json": {
              schema: {
                $ref: "#/components/schemas/RegisterSessionRequest",
              },
            },
          },
        },
        responses: {
          "200": {
            description: "Registered verifier session.",
            content: {
              "application/json": {
                schema: {
                  $ref: "#/components/schemas/RegisterSessionResponse",
                },
              },
            },
          },
          "400": {
            $ref: "#/components/responses/Error",
          },
          "409": {
            $ref: "#/components/responses/Error",
          },
          "500": {
            $ref: "#/components/responses/Error",
          },
        },
      },
    },
    "/v1/sessions/{sessionId}/verifier": {
      get: {
        operationId: "connectVerifierWebSocket",
        summary: "Connect verifier WebSocket",
        description:
          "The prover connects with a WebSocket upgrade. Binary and text frames are bridged into the TLSN verifier runtime.",
        parameters: [
          {
            $ref: "#/components/parameters/SessionId",
          },
        ],
        responses: {
          "101": {
            description: "WebSocket upgrade accepted.",
          },
          "404": {
            $ref: "#/components/responses/Error",
          },
          "409": {
            $ref: "#/components/responses/Error",
          },
          "500": {
            $ref: "#/components/responses/Error",
          },
        },
      },
    },
    "/v1/sessions/{sessionId}/reveal-config": {
      post: {
        operationId: "submitRevealConfig",
        summary: "Submit reveal config",
        parameters: [
          {
            $ref: "#/components/parameters/SessionId",
          },
        ],
        requestBody: {
          required: true,
          content: {
            "application/json": {
              schema: {
                $ref: "#/components/schemas/RevealConfig",
              },
            },
          },
        },
        responses: {
          "200": {
            description: "Reveal config accepted.",
            content: {
              "application/json": {
                schema: {
                  $ref: "#/components/schemas/OkSessionResponse",
                },
              },
            },
          },
          "400": {
            $ref: "#/components/responses/Error",
          },
          "404": {
            $ref: "#/components/responses/Error",
          },
          "500": {
            $ref: "#/components/responses/Error",
          },
        },
      },
    },
    "/v1/sessions/{sessionId}/attestation": {
      post: {
        operationId: "issueAttestation",
        summary: "Issue attestation",
        parameters: [
          {
            $ref: "#/components/parameters/SessionId",
          },
        ],
        requestBody: {
          required: true,
          content: {
            "application/json": {
              schema: {
                $ref: "#/components/schemas/AttestationIssueRequest",
              },
            },
          },
        },
        responses: {
          "200": {
            description: "Issued attestation.",
            content: {
              "application/json": {
                schema: {
                  $ref: "#/components/schemas/AttestationIssueResponse",
                },
              },
            },
          },
          "400": {
            $ref: "#/components/responses/Error",
          },
          "404": {
            $ref: "#/components/responses/Error",
          },
          "409": {
            $ref: "#/components/responses/Error",
          },
          "500": {
            $ref: "#/components/responses/Error",
          },
        },
      },
    },
    "/v1/sessions/{sessionId}/result": {
      get: {
        operationId: "getSessionResult",
        summary: "Get session result",
        parameters: [
          {
            $ref: "#/components/parameters/SessionId",
          },
        ],
        responses: {
          "200": {
            description: "Verifier session state and optional result metadata.",
            content: {
              "application/json": {
                schema: {
                  $ref: "#/components/schemas/SessionResultResponse",
                },
              },
            },
          },
          "404": {
            $ref: "#/components/responses/Error",
          },
          "500": {
            $ref: "#/components/responses/Error",
          },
        },
      },
    },
    "/v1/artifacts/verify": {
      post: {
        operationId: "verifyArtifact",
        summary: "Verify proof artifact",
        requestBody: {
          required: true,
          content: {
            "application/json": {
              schema: {
                $ref: "#/components/schemas/ArtifactVerifyRequest",
              },
            },
          },
        },
        responses: {
          "200": {
            description: "Artifact verification result.",
            content: {
              "application/json": {
                schema: {
                  $ref: "#/components/schemas/ArtifactVerifyResponse",
                },
              },
            },
          },
          "400": {
            $ref: "#/components/responses/Error",
          },
          "500": {
            $ref: "#/components/responses/Error",
          },
        },
      },
    },
  },
  components: {
    parameters: {
      SessionId: {
        name: "sessionId",
        in: "path",
        required: true,
        schema: {
          type: "string",
        },
      },
    },
    responses: {
      Error: {
        description: "Error response.",
        content: {
          "application/json": {
            schema: {
              $ref: "#/components/schemas/ErrorResponse",
            },
          },
        },
      },
    },
    schemas: {
      InfoResponse: {
        type: "object",
        required: ["service", "apiVersion"],
        properties: {
          service: {
            type: "string",
            example: "mobile_tlsn_verifier_service",
          },
          apiVersion: {
            type: "string",
            example: "v1",
          },
        },
      },
      RegisterSessionRequest: {
        type: "object",
        required: ["clientSessionId", "maxRecvData", "maxSentData"],
        properties: {
          sessionId: {
            type: "string",
            format: "uuid",
            nullable: true,
            description:
              "Optional Worker-minted UUID. When omitted, the service generates one.",
          },
          clientSessionId: {
            type: "string",
          },
          maxRecvData: {
            type: "integer",
            minimum: 0,
          },
          maxSentData: {
            type: "integer",
            minimum: 0,
          },
          sessionData: {
            type: "object",
            additionalProperties: {
              type: "string",
            },
            default: {},
            description:
              "String map containing reveal policy values such as allowedRequestHeaders and allowedResponseBodyJsonPaths as JSON-encoded string arrays.",
            example: {
              allowedRequestHeaders: "[\"accept\"]",
              allowedResponseBodyJsonPaths: "[\"user.name\"]",
            },
          },
        },
      },
      RegisterSessionResponse: {
        type: "object",
        required: ["sessionId", "verifierWsUrl"],
        properties: {
          sessionId: {
            type: "string",
          },
          verifierWsUrl: {
            type: "string",
            description:
              "Relative WebSocket endpoint path. Resolve it against the same origin used for registration, with ws/wss matching http/https.",
            example: "/v1/sessions/00000000-0000-4000-8000-000000000000/verifier",
          },
        },
      },
      RevealConfig: {
        type: "object",
        required: ["sent", "recv"],
        properties: {
          sent: {
            type: "array",
            items: {
              $ref: "#/components/schemas/RangeWithHandler",
            },
          },
          recv: {
            type: "array",
            items: {
              $ref: "#/components/schemas/RangeWithHandler",
            },
          },
        },
      },
      RangeWithHandler: {
        type: "object",
        required: ["start", "end", "handler"],
        properties: {
          start: {
            type: "integer",
            minimum: 0,
          },
          end: {
            type: "integer",
            minimum: 0,
          },
          handler: {
            $ref: "#/components/schemas/Handler",
          },
        },
      },
      Handler: {
        type: "object",
        required: ["type", "part"],
        properties: {
          type: {
            $ref: "#/components/schemas/HandlerType",
          },
          part: {
            $ref: "#/components/schemas/HandlerPart",
          },
          params: {
            $ref: "#/components/schemas/HandlerParams",
          },
        },
      },
      HandlerType: {
        type: "string",
        enum: ["SENT", "RECV"],
      },
      HandlerPart: {
        type: "string",
        enum: [
          "START_LINE",
          "PROTOCOL",
          "METHOD",
          "REQUEST_TARGET",
          "STATUS_CODE",
          "HEADERS",
          "BODY",
          "ALL",
        ],
      },
      HandlerParams: {
        type: "object",
        properties: {
          type: {
            type: "string",
            nullable: true,
            description:
              "Alias for content type. The verifier currently expects `json` for received BODY entries.",
          },
          contentType: {
            type: "string",
            nullable: true,
          },
          path: {
            type: "string",
            nullable: true,
          },
        },
      },
      AttestationIssueRequest: {
        type: "object",
        required: ["attestationRequestB64"],
        properties: {
          attestationRequestB64: {
            type: "string",
            format: "byte",
          },
        },
      },
      AttestationIssueResponse: {
        type: "object",
        required: ["attestationB64"],
        properties: {
          attestationB64: {
            type: "string",
            format: "byte",
          },
        },
      },
      SessionResultResponse: {
        type: "object",
        required: [
          "status",
          "sessionId",
          "clientSessionId",
          "createdAtUtc",
          "revealConfigReceived",
        ],
        properties: {
          status: {
            type: "string",
            enum: ["pending", "in_progress", "completed", "failed"],
          },
          sessionId: {
            type: "string",
          },
          clientSessionId: {
            type: "string",
          },
          createdAtUtc: {
            type: "string",
            format: "date-time",
          },
          revealConfigReceived: {
            type: "boolean",
          },
          result: {
            allOf: [
              {
                $ref: "#/components/schemas/SessionResult",
              },
            ],
            nullable: true,
          },
          errorMessage: {
            type: "string",
            nullable: true,
          },
        },
      },
      SessionResult: {
        type: "object",
        required: [
          "serverName",
          "transcriptSentLen",
          "transcriptRecvLen",
          "transcriptSentDigestBase64",
          "transcriptRecvDigestBase64",
          "verifiedAtUtc",
        ],
        properties: {
          serverName: {
            type: "string",
          },
          transcriptSentLen: {
            type: "integer",
            minimum: 0,
          },
          transcriptRecvLen: {
            type: "integer",
            minimum: 0,
          },
          transcriptSentDigestBase64: {
            type: "string",
          },
          transcriptRecvDigestBase64: {
            type: "string",
          },
          verifiedAtUtc: {
            type: "string",
            format: "date-time",
          },
        },
      },
      ArtifactVerifyRequest: {
        type: "object",
        required: ["artifact"],
        properties: {
          artifact: {
            type: "object",
            additionalProperties: true,
            description:
              "Plugin proof artifact. The service validates the nested payload, proof envelope, notary key policy, and presentation signature.",
          },
        },
      },
      ArtifactVerifyResponse: {
        type: "object",
        required: ["integrityVerified", "semanticVerified"],
        properties: {
          integrityVerified: {
            type: "boolean",
          },
          semanticVerified: {
            type: "boolean",
          },
          reason: {
            type: "string",
            nullable: true,
          },
        },
      },
      OkSessionResponse: {
        type: "object",
        required: ["ok", "sessionId"],
        properties: {
          ok: {
            type: "boolean",
            example: true,
          },
          sessionId: {
            type: "string",
          },
        },
      },
      ErrorResponse: {
        type: "object",
        required: ["code", "message"],
        properties: {
          code: {
            type: "string",
          },
          message: {
            type: "string",
          },
        },
      },
    },
  },
} as const;
