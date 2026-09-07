import { expect, test } from "@playwright/test";
import { testPort } from "../test-port";

test("all preview and dev servers validate their port overrides", () => {
  const variables: ("BREWTHINK_WEB_PORT" | "BREWTHINK_PARITY_PORT")[] = [
    "BREWTHINK_WEB_PORT", "BREWTHINK_PARITY_PORT",
  ];
  for (const variable of variables) {
    const original = process.env[variable];
    try {
      delete process.env[variable];
      expect(testPort(variable, 4185)).toBe(4185);
      expect(() => testPort(variable, 0)).toThrow(`${variable} must be a TCP port`);
      for (const value of ["1", "4173", "65535"]) {
        process.env[variable] = value;
        expect(testPort(variable, 4185)).toBe(Number(value));
      }
      for (const value of ["", "0", "65536", "NaN", "2.5", "-1", "1; echo wrong", "1e309"]) {
        process.env[variable] = value;
        expect(() => testPort(variable, 4185)).toThrow(`${variable} must be a TCP port`);
      }
    } finally {
      if (original === undefined) delete process.env[variable];
      else process.env[variable] = original;
    }
  }
});
