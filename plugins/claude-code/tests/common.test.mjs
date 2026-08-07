import { test, describe } from "node:test";
import assert from "node:assert/strict";
import {
  cleanText,
  formatRelativeAge,
  truncateQuery,
  sanitizeContent,
} from "../hooks/common.mjs";

describe("cleanText", () => {
  test("strips cerebro inject tags", () => {
    const input = "hello <cerebro-memory>secret</cerebro-memory> world";
    assert.equal(cleanText(input), "hello world");
  });

  test("strips cerebro self-closing tags", () => {
    const input = "text <cerebro-nudge /> after";
    assert.equal(cleanText(input), "text after");
  });

  test("strips system-reminder tags", () => {
    const input = "before <system-reminder>internal</system-reminder> after";
    assert.equal(cleanText(input), "before after");
  });

  test("strips supermemory tags", () => {
    const input = "a <supermemory-context>data</supermemory-context> b";
    assert.equal(cleanText(input), "a b");
  });

  test("collapses whitespace", () => {
    const input = "line1\n\n\n  line2   \n\tline3";
    assert.equal(cleanText(input), "line1 line2 line3");
  });

  test("handles non-string input", () => {
    assert.equal(cleanText(42), "42");
    assert.equal(cleanText(null), "null");
  });
});

describe("formatRelativeAge", () => {
  test("returns 'unknown' for null/undefined", () => {
    assert.equal(formatRelativeAge(null), "unknown");
    assert.equal(formatRelativeAge(undefined), "unknown");
    assert.equal(formatRelativeAge(""), "unknown");
  });

  test("returns minutes for recent dates", () => {
    const fiveMinAgo = new Date(Date.now() - 5 * 60000).toISOString();
    const result = formatRelativeAge(fiveMinAgo);
    assert.match(result, /^\d+m ago$/);
  });

  test("returns hours for older dates", () => {
    const threeHrAgo = new Date(Date.now() - 3 * 3600000).toISOString();
    const result = formatRelativeAge(threeHrAgo);
    assert.match(result, /^\d+h ago$/);
  });

  test("returns days for dates > 24h", () => {
    const threeDayAgo = new Date(Date.now() - 3 * 86400000).toISOString();
    const result = formatRelativeAge(threeDayAgo);
    assert.match(result, /^\d+d ago$/);
  });

  test("returns months for dates > 30d", () => {
    const twoMonthAgo = new Date(Date.now() - 70 * 86400000).toISOString();
    const result = formatRelativeAge(twoMonthAgo);
    assert.match(result, /^\d+mo ago$/);
  });
});

describe("truncateQuery", () => {
  test("returns empty for empty input", () => {
    assert.equal(truncateQuery(""), "");
    assert.equal(truncateQuery(null), "");
    assert.equal(truncateQuery(undefined), "");
  });

  test("returns short query unchanged", () => {
    assert.equal(truncateQuery("hello"), "hello");
  });

  test("truncates long query to default length", () => {
    const long = "x".repeat(300);
    const result = truncateQuery(long);
    assert.equal(result.length, 200);
  });

  test("respects custom length", () => {
    const long = "x".repeat(100);
    assert.equal(truncateQuery(long, 50).length, 50);
  });
});

describe("sanitizeContent", () => {
  test("removes HTML tags", () => {
    const input = "text <div>inner</div> more";
    assert.equal(sanitizeContent(input, 1000), "text more");
  });

  test("removes self-closing tags", () => {
    const input = "a <br/> b";
    assert.equal(sanitizeContent(input, 1000), "a b");
  });

  test("collapses whitespace", () => {
    const input = "line1\n\n  line2";
    assert.equal(sanitizeContent(input, 1000), "line1 line2");
  });

  test("truncates with ellipsis marker when exceeding maxLen", () => {
    const long = "x".repeat(200);
    const result = sanitizeContent(long, 50);
    assert.ok(result.length <= 65); // 50 + "…[truncated]"
    assert.ok(result.includes("…[truncated]"));
  });

  test("does not truncate short content", () => {
    assert.equal(sanitizeContent("short", 1000), "short");
  });
});
