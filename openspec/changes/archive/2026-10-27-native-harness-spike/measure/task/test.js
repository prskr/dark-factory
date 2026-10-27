const assert = require("assert");
const slugify = require("./slug");
assert.strictEqual(slugify("Hello World"), "hello-world");
assert.strictEqual(slugify("  Foo   Bar!! "), "foo-bar");
assert.strictEqual(slugify("a--b"), "a-b");
assert.strictEqual(slugify("---"), "");
console.log("ok");
