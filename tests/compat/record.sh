#!/usr/bin/env bash
# Record the golden compatibility cases from a sed-rs binary.
#
#   tests/compat/record.sh path/to/sed-rs
#
# Each case is a directory under tests/compat/cases/ holding:
#   args    the arguments, NUL-separated
#   stdin   the input bytes
#   stdout  the output bytes the binary produced
#   status  its exit status
#
# The committed cases were recorded from v0.2.0 (edf6b73). Re-record only to
# add new cases, and never change an existing one without a "Behaviour
# changes" entry in CHANGELOG.md. tests/compat.rs replays them.
set -euo pipefail
bin=$1
dir=$(cd "$(dirname "$0")" && pwd)/cases

case_() { # case_ NAME STDIN ARG...
    local name=$1 input=$2; shift 2
    local d=$dir/$name
    mkdir -p "$d"
    printf '%s\0' "$@" > "$d/args"
    printf '%b' "$input" > "$d/stdin"
    set +e
    "$bin" "$@" < "$d/stdin" > "$d/stdout" 2>/dev/null
    echo $? > "$d/status"
    set -e
}

# Real invocations: reva-fork scripts/doc-check.sh, parity-examples.sh
case_ reva-trailing-space   'a  \nb\t\n\n' -e 's/[[:space:]]*$//'
case_ reva-trailing-blank   'x\n\ny\n\n\n' -e ':a' -e '/^\\n*$/{$d;N;ba' -e '}'
case_ reva-trailing-blank-2 '\n\nx\n' -e ':a' -e '/^\\n*$/{$d;N;ba' -e '}'
case_ reva-trailing-blank-3 '\n\n' -e ':a' -e '/^\\n*$/{$d;N;ba' -e '}'
case_ reva-indent           'a\nb\n' -e 's/^/    /'
case_ reva-fault            '*** bad word (in x.f, eip 12)  \n' -e 's/^\*\*\* //; s/ [(]in [^()]*, eip [^)]*[)]$//; s/[[:space:]]*$//'
case_ reva-missing          'foo?\n' -e 's/[?]$//'
case_ reva-error            'ERROR: a   b  \n' -e 's/^ERROR: //; s/[[:space:]]+/ /g; s/ *$//'
case_ reva-colon            'runs: x\nmissing: y\n' -e 's/:.*//'
# Real invocations: forthlet scripts/test.sh, run-flt.sh
case_ forthlet-uart  'pre UART output:   1  2 Executed 9\n' -e 's/.*UART output: //' -e 's/Executed.*//' -e 's/  */ /g' -e 's/^ //;s/ $//'
case_ forthlet-reg   'REG Power 2 n e 1 p  a  b \n' -e 's/REG Power 2 n e 1 p //' -e 's/  */ /g' -e 's/^ //;s/ $//'
case_ forthlet-addr  '0000ABCD foo\n' -e 's/^[0-9A-F]{8} /ADDR     /'
# Documented examples
case_ doc-dates      'on 2026-03-25 and 2025-01-02\n' -e 's/[0-9]{4}-[0-9]{2}-[0-9]{2}/DATE/g'
case_ doc-bre-dates  'on 2026-03-25\n' -e 's/[0-9]\{4\}-[0-9]\{2\}-[0-9]\{2\}/DATE/g'
case_ doc-tmp        'x /tmp/abc y\n' -e 's|/tmp/[^ ]*|<TMPDIR>|g'
case_ doc-ci         'Hello HELLO hello\n' -e 's/hello/hi/gi'
case_ doc-captures   'John Smith\n' -e 's/(\w+) (\w+)/\2, \1/'
case_ doc-bre-groups 'hello world\n' -e 's/\(hello\) \(world\)/\2 \1/'
case_ doc-join       '1\n2\n3\n' -e ':a' -e 'N' -e '$!ba' -e 's/\\n/,/g'
case_ doc-last-del   'a\nb\nc\n' -e '$d'
case_ doc-debug-del  'DEBUG x\ny\n' -e '/^DEBUG/d'
case_ doc-negate     'keep 1\ndrop\nkeep 2\n' -e '/keep/!d'
case_ doc-block      'ax\naa\n' -e '/x/{s/x/y/;s/a/b/}'
case_ doc-comment    'a\nb\n' -e '# drop b
/b/d ;  s/a/A/'
# Byte-level behaviour of the stream mode (frozen)
case_ eol-crlf       'a\r\nb\r\n' -e 's/a/x/'
case_ eol-no-final   'a\nb' -e 's/b/y/'
case_ eol-empty      '' -e 's/a/b/'
case_ eol-cr-only    'a\rb\n' -e 's/a/x/'
case_ utf8-invalid   'ok\n\xff\nafter\n' -e 's/o/0/'
case_ utf8-multibyte 'héllo wörld\n' -e 's/ö/o/; s/^./E/'
case_ err-numeric-flag 'abc\n' -e 's/./E/2'
case_ flag-p 'aa\n' -e 's/a/b/p'
case_ flag-L 'a.b axb\n' -e 's/a.b/X/gL'
# Edge cases of the parser and replacement syntax (frozen)
case_ empty-regex    'abc\n' -e 's//X/'
case_ empty-regex-g  'ab\n' -e 's//-/g'
case_ line-zero      '1\n2\n' -e '0d'
case_ n-last-line    'a\nb\nc\n' -e 'N;s/\\n/+/'
case_ n-not-last     'a\nb\nc\n' -e '$!N;s/\\n/+/'
case_ dollar-repl    'the price\n' -e 's/price/\\$5/'
case_ dollar-bare    'x\n' -e 's/x/$1$/'
case_ amp-escaped    'a\n' -e 's/a/[\\&&]/'
case_ backslash-repl 'a\n' -e 's/a/\\\\/'
case_ tab-repl       'a\n' -e 's/a/1\\t2/'
case_ newline-repl   'a\nb\n' -e 's/a/a\\nnew/'
case_ esc-delim      'a/b\n' -e 's/a\/b/c/'
case_ semicolon-pat  'xa;by\n' -e 's/a;b/c/'
case_ missing-delim  'ab\n' -e 's/a/b' -e 's/b/c/g'
case_ hash-delim     'a#b\n' -e 's#a\#b#X#'
case_ bre-plus       'aaa+\n' -e 's/a\+/X/'
case_ bre-qmark      'ab\n' -e 's/ab\?/X/'
case_ bre-alt        'cat dog\n' -e 's/cat\|dog/X/g'
case_ dot-escaped    'a.b axb\n' -e 's/a\.b/X/g'
case_ upper-i-flag   'ABC\n' -e 's/b/x/I'
case_ branch-end     'a\n' -e 'b;s/a/b/'
case_ label-skip     'a\nb\n' -e '/a/bend' -e 's/./X/' -e ':end'
case_ ws-around-addr 'a\nb\n' -e '2 ! d'
case_ nested-block   'ab\ncd\n' -e '/a/{/b/{s/a/X/}}'
case_ addr-with-n    'x\na\nb\n' -e '/^a/N;s/\\n/=/'
case_ multi-e-block  'a\nb\n' -e '/a/{' -e 's/a/A/' -e '}'
# Errors (exit 1, nothing on stdout)
case_ err-no-expr    'a\n'
case_ err-label      'a\n' -e 'ba'
case_ err-unmatched  'a\n' -e '/x/{s/a/b/'
case_ err-close      'a\n' -e '}'
case_ err-y          'a\n' -e 'y/abc/xyz/'
case_ err-colon      'a\n' -e ':'
case_ err-addr       'a\n' -e '/x'
case_ err-regex      'a\n' -e 's/(/x/'
case_ err-subst      'a\n' -e 's/a'
