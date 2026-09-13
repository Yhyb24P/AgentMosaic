const release = "https://github.com/Yhyb24P/AgentMosaic/releases/latest";
const installer = "https://github.com/Yhyb24P/AgentMosaic/releases/latest/download/agentmosaic-cli-installer.sh";
export default { async fetch(request: Request, env: Env): Promise<Response> {
  const path = new URL(request.url).pathname;
  if (path === "/install.sh") return Response.redirect(installer, 302);
  if (path === "/release") return Response.redirect(release, 302);
  return env.ASSETS.fetch(request);
}} satisfies ExportedHandler<Env>;
