const release = "https://github.com/Yhyb24P/AgentMosaic/releases/latest";
export default { async fetch(request: Request, env: Env): Promise<Response> {
  const path = new URL(request.url).pathname;
  if (path === "/install.sh") return new Response("AgentMosaic v0.2 installer is not published yet. No software was installed.\n", {status: 503, headers: {"content-type":"text/plain; charset=utf-8","cache-control":"no-store"}});
  if (path === "/release") return Response.redirect(release, 302);
  return env.ASSETS.fetch(request);
}} satisfies ExportedHandler<Env>;
