// Thin Netlify proxy for a separately hosted, authenticated Oorush HTTP gateway.
// Netlify cannot reach a node listening on 127.0.0.1, and the Rust MVP speaks
// newline-delimited TCP JSON. Set OORUSH_STATUS_URL to an HTTPS gateway that
// exposes a read-only, normalized status document; never put node credentials in
// this function or in the browser.

exports.handler = async function handler(event) {
  if (event.httpMethod !== "GET") {
    return response(405, { error: "method_not_allowed" }, { Allow: "GET" });
  }

  const upstream = process.env.OORUSH_STATUS_URL;
  if (!upstream) {
    return response(503, {
      error: "status_source_not_configured",
      message: "Set OORUSH_STATUS_URL to an HTTPS read-only status gateway.",
    });
  }

  let url;
  try {
    url = new URL(upstream);
  } catch {
    return response(500, { error: "invalid_status_source" });
  }
  if (url.protocol !== "https:") {
    return response(500, { error: "status_source_must_use_https" });
  }

  try {
    const upstreamResponse = await fetch(url, {
      headers: { Accept: "application/json" },
      signal: AbortSignal.timeout(5000),
    });
    const body = await upstreamResponse.text();
    if (!upstreamResponse.ok) {
      return response(502, { error: "upstream_unavailable", status: upstreamResponse.status });
    }
    const data = JSON.parse(body);
    return response(200, data, { "Cache-Control": "no-store" });
  } catch {
    return response(502, { error: "upstream_unavailable" });
  }
};

function response(statusCode, body, extraHeaders = {}) {
  return {
    statusCode,
    headers: {
      "Content-Type": "application/json; charset=utf-8",
      "Access-Control-Allow-Origin": "*",
      ...extraHeaders,
    },
    body: JSON.stringify(body),
  };
}
