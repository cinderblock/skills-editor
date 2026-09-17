import { expect, test } from "bun:test";
import { baseName, dirName, isUnder, joinPath, relativeTo, sepOf } from "./paths";

test("separator is taken from the path itself", () => {
  expect(sepOf("C:\\Users\\me\\.claude")).toBe("\\");
  expect(sepOf("/home/me/.claude")).toBe("/");
  expect(sepOf("")).toBe("/");
  // Mixed input (a Windows path with a forward slash in it) still joins with \.
  expect(sepOf("C:\\Users\\me/skills")).toBe("\\");
});

test("join keeps each platform's style", () => {
  expect(joinPath("C:\\skills\\demo", "scripts/run.sh")).toBe("C:\\skills\\demo\\scripts\\run.sh");
  expect(joinPath("/home/me/skills/demo", "scripts/run.sh")).toBe("/home/me/skills/demo/scripts/run.sh");
  expect(joinPath("C:\\skills\\demo\\", "a.md")).toBe("C:\\skills\\demo\\a.md");
  expect(joinPath("/home/me/", "a.md")).toBe("/home/me/a.md");
});

test("containment needs a separator boundary", () => {
  expect(isUnder("C:\\skills\\demo\\a.md", "C:\\skills\\demo")).toBe(true);
  expect(isUnder("/home/me/demo/a.md", "/home/me/demo")).toBe(true);
  expect(isUnder("/home/me/demo", "/home/me/demo")).toBe(false);
  expect(isUnder("/home/me/demo-2/a.md", "/home/me/demo")).toBe(false);
  expect(isUnder(null, "/home/me")).toBe(false);
});

test("base, dir, and relative names", () => {
  expect(baseName("C:\\a\\b.md")).toBe("b.md");
  expect(baseName("/a/b.md")).toBe("b.md");
  expect(baseName("b.md")).toBe("b.md");
  expect(dirName("C:\\a\\b.md")).toBe("C:\\a");
  expect(dirName("/a/b.md")).toBe("/a");
  expect(dirName("b.md")).toBe("");
  expect(relativeTo("C:\\a\\sub\\b.md", "C:\\a")).toBe("sub/b.md");
  expect(relativeTo("/a/sub/b.md", "/a")).toBe("sub/b.md");
  expect(relativeTo("/other/b.md", "/a")).toBe("/other/b.md");
});
