const swc = require("../../");

it("should handle minify", () => {
  const src = '/* Comment */import foo, {bar} from "foo"';

  expect(
    swc
      .transformSync(src, {
        minify: true
      })
      .code.trim()
  ).toBe("import foo,{bar}from'foo';");
});

it("should handle sourceMaps = false", () => {
  const src = '/* Comment */import foo, {bar} from "foo"';
  const out = swc.transformSync(src, {
    sourceMaps: false
  });

  expect(out.map).toBeFalsy();
});

it("should handle exportNamespaceFrom", () => {
  const src = "export * as Foo from 'bar';";
  const out = swc.transformSync(src, {
    jsc: {
      parser: {
        syntax: "ecmascript",
        exportNamespaceFrom: true
      }
    }
  });

  expect(out.code).toContain("import * as _Foo from 'bar';");
  expect(out.code).toContain("export { _Foo as Foo }");
});

it("should handle jsc.target = es3", () => {
  const out = swc.transformSync(`foo.default`, {
    jsc: {
      target: "es3"
    }
  });
  expect(out.code.trim()).toBe(`foo['default'];`);
});

it("should handle jsc.target = es5", () => {
  const out = swc.transformSync(`foo.default`, {
    jsc: {
      target: "es5"
    }
  });
  expect(out.code.trim()).toBe(`foo.default;`);
});

it("(sync) should handle module input", () => {
  const m = swc.parseSync("class Foo {}");
  const out = swc.transformSync(m);

  expect(out.code.replace(/\n/g, "")).toBe("class Foo{}");
});

it("(async) should handle module input", async () => {
  const m = await swc.parse("class Foo {}");
  const out = await swc.transform(m);

  expect(out.code.replace(/\n/g, "")).toBe("class Foo{}");
});

it("(sync) should handle plugin", () => {
  const out = swc.transformSync("class Foo {}", {
    plugin: m => ({ ...m, body: [] })
  });

  expect(out.code).toBe("");
});

it("(async) should handle plugin", async () => {
  const out = await swc.transform("class Foo {}", {
    plugin: m => ({ ...m, body: [] })
  });

  expect(out.code).toBe("");
});
