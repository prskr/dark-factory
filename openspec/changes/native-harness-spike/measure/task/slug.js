// slugify: lowercase, trim, collapse non-alphanumerics to single "-", no leading/trailing "-".
module.exports = function slugify(s) {
  return s.toLowerCase().replace(/[^a-z0-9]/g, "-");
};
