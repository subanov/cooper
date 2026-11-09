#!/usr/bin/env bash
# Markdown summary of an lcov file: line coverage overall and of the domain layer per
# crate (`src/domain/` or `src/domain.rs`), which AGENTS.md §7 expects to be covered
# best. Used by CI (#84); run locally after `just coverage`.
#
#   scripts/coverage-summary.sh lcov.info
set -euo pipefail
lcov="${1:-lcov.info}"
totals() {
  awk '
    /^SF:/ {
      file = substr($0, 4)
      crate = file
      sub(/.*\/(crates|apps)\//, "", crate); sub(/\/.*/, "", crate)
      domain = (file ~ /\/src\/domain\// || file ~ /\/src\/domain\.rs$/)
    }
    /^LF:/ { lf = substr($0, 4); all_lf += lf; if (domain) { dlf[crate] += lf; dom_lf += lf } }
    /^LH:/ { lh = substr($0, 4); all_lh += lh; if (domain) { dlh[crate] += lh; dom_lh += lh } }
    function pct(h, f) { return f ? sprintf("%.1f%%", 100 * h / f) : "n/a" }
    END {
      printf "0|| **All code** | %d | %d | **%s** |\n", all_lh, all_lf, pct(all_lh, all_lf)
      printf "1|| **Domain layers** | %d | %d | **%s** |\n", dom_lh, dom_lf, pct(dom_lh, dom_lf)
      for (c in dlf) printf "2%s|| `%s` domain | %d | %d | %s |\n", c, c, dlh[c], dlf[c], pct(dlh[c], dlf[c])
    }
  ' "$lcov"
}

echo "### Test coverage (lines)"
echo
echo "| Scope | Covered | Lines | Coverage |"
echo "|---|---:|---:|---:|"
totals | LC_ALL=C sort | sed 's/^[^|]*|//'
