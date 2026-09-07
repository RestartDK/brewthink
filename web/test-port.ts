export function testPort(variable: "BREWTHINK_WEB_PORT" | "BREWTHINK_PARITY_PORT", fallback: number): number {
  const port = Number(process.env[variable] ?? fallback);
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    throw new Error(`${variable} must be a TCP port`);
  }
  return port;
}
