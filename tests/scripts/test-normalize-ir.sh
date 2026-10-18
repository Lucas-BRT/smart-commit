#!/usr/bin/env bash
# The same graph written with different node counters must normalize to the
# same bytes, and names OpenVINO did not generate must stay as they are.
#
#   tests/scripts/test-normalize-ir.sh
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

ir() {
    cat <<XML
<net name="Model0" version="11">
	<layers>
		<layer id="0" name="input_ids" type="Parameter" version="opset1" />
		<layer id="1" name="Convert_$1" type="Convert" version="opset1" />
		<layer id="2" name="Constant_$2" type="Const" version="opset1" />
		<layer id="3" name="Convert_$3" type="Convert" version="opset1" />
		<layer id="4" name="__module.model.layers.0/aten::add/Add" type="Add" version="opset1" />
	</layers>
	<rt_info><info name="OpenVINO Runtime" value="2026.3.1" /></rt_info>
</net>
XML
}

ir 110687 557874 110690 >"$work/a.xml"
ir 110688 557960 110691 >"$work/b.xml"
python3 -I "$ROOT/scripts/export/normalize_ir.py" "$work/a.xml" "$work/b.xml"

cmp "$work/a.xml" "$work/b.xml"
echo "ok    different counters give the same file"
grep -q 'name="Convert_0"' "$work/a.xml"
grep -q 'name="Constant_1"' "$work/a.xml"
grep -q 'name="Convert_2"' "$work/a.xml"
echo "ok    auto names are numbered in order of appearance"
grep -q 'name="input_ids"' "$work/a.xml"
grep -q 'name="__module.model.layers.0/aten::add/Add"' "$work/a.xml"
grep -q 'name="OpenVINO Runtime"' "$work/a.xml"
echo "ok    other names are kept"
