import { readFileSync } from "node:fs";
import ts from "typescript";
import { describe, expect, it } from "vitest";

function attributesFor(relativePath: string, tag: string) {
  const source = readFileSync(new URL(relativePath, import.meta.url), "utf8");
  const ast = ts.createSourceFile(
    relativePath,
    source,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TSX,
  );
  const found: ts.JsxAttributes[] = [];
  const visit = (node: ts.Node) => {
    if (
      (ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) &&
      node.tagName.getText(ast) === tag
    )
      found.push(node.attributes);
    ts.forEachChild(node, visit);
  };
  visit(ast);
  return found.map((attributes) =>
    Object.fromEntries(
      attributes.properties
        .filter(ts.isJsxAttribute)
        .map((attribute) => [
          attribute.name.getText(ast),
          attribute.initializer && ts.isStringLiteral(attribute.initializer)
            ? attribute.initializer.text
            : null,
        ]),
    ),
  );
}

describe("shared bilingual root consent policy", () => {
  it("mounts one consent-gated first-party Web Vitals client in the shared root", () => {
    expect(
      attributesFor(
        "../../components/layout/root-document.tsx",
        "ConsentWebVitals",
      ),
    ).toHaveLength(1);
  });
  it.each(["nb", "en"])(
    "uses the reviewed single consent authority in the %s root",
    (locale) => {
      const roots = attributesFor(
        `../../app/(${locale})/layout.tsx`,
        "RootDocument",
      );
      expect(roots).toEqual([{ locale }]);
      const scripts = attributesFor(
        "../../components/layout/root-document.tsx",
        "Script",
      ).filter((attributes) => attributes.id === "Cookiebot");
      expect(scripts).toHaveLength(1);
      expect(scripts[0]["data-consentmode"]).toBe("disabled");
      expect(scripts[0]["data-blockingmode"]).toBe("auto");
    },
  );
});
