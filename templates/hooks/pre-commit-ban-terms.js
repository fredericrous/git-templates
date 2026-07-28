#!/usr/bin/env node
/**
 * Prevent commit of forbidden terms
 *
 * Two-stage on purpose:
 *
 *   1. `git diff -G` picks the candidate files cheaply, and keeps the check
 *      scoped to what this commit actually touches — a pre-existing violation
 *      in an untouched part of a file you edited is not this commit's problem.
 *   2. Each candidate is re-checked precisely against its STAGED content, with
 *      comments and string literals blanked out.
 *
 * Stage 1's patterns are deliberately loose: they are POSIX regexes handed to
 * git, whose flavour varies by platform. Stage 2 is where correctness lives,
 * using real JS regexes. A loose prefilter costs at most one extra file read.
 *
 * Stage 2 removes three classes of false positive the diff scan produced on
 * its own:
 *   - `describe.skipIf(...)` is not `describe.skip`. The ban is on an exact
 *     identifier, so a longer one that merely starts with it must pass.
 *   - a term named in a comment (or a string) is discussion, not code.
 *   - REMOVING a line containing `debugger;` matched `-G` exactly as adding
 *     one did, so deleting a violation was reported as committing one.
 *
 * Author: https://github.com/fredericrous
 */
const { execSync, execFileSync } = require('child_process');
const { EOL } = require('os');
const path = require('path');

const FILES_TO_SEARCH_IN = /\.(jsx?|tsx?|vue)$/;

/**
 * `git` — loose prefilter for `git diff -G`. Must never be stricter than its
 *         `js` counterpart, or real violations slip through unread.
 * `js`   — the actual rule. The lookbehind keeps `profit(` and `foo.fit(` out;
 *          the trailing guard keeps `skipIf` and friends out.
 */
const TERMS = {
  fit: {
    git: '\\s*fit\\(',
    js: /(?<![\w.$])fit\s*\(/,
  },
  fdescribe: {
    git: '\\s*fdescribe\\(',
    js: /(?<![\w.$])fdescribe\s*\(/,
  },
  debugger: {
    git: 'debugger;?',
    js: /(?<![\w.$])debugger(?![\w$])/,
  },
  skipOnly: {
    git: '(describe|context|it)\\.(skip|only)',
    js: /(?<![\w$])(describe|context|it)\.(skip|only)(?![\w$])/,
  },
};

/**
 * Blank out comments and the insides of string/template literals, leaving a
 * string of the same length and line count so offsets still line up. Blanked
 * rather than deleted for that reason.
 *
 * Not a parser. A regex literal containing an escaped slash can be misread as
 * a comment opener, which over-blanks — costing a missed warning rather than a
 * false alarm. That is the right way round for a hook standing between someone
 * and their commit.
 */
function blankNonCode(src) {
  const CODE = 0;
  const LINE = 1;
  const BLOCK = 2;
  const SINGLE = 3;
  const DOUBLE = 4;
  const TEMPLATE = 5;

  const keepLayout = (ch) => (ch === '\n' ? '\n' : ' ');

  let out = '';
  let state = CODE;
  let i = 0;

  while (i < src.length) {
    const ch = src[i];
    const next = src[i + 1];

    if (state === CODE) {
      if (ch === '/' && next === '/') {
        state = LINE;
        out += '  ';
        i += 2;
      } else if (ch === '/' && next === '*') {
        state = BLOCK;
        out += '  ';
        i += 2;
      } else if (ch === "'" || ch === '"' || ch === '`') {
        state = ch === "'" ? SINGLE : ch === '"' ? DOUBLE : TEMPLATE;
        out += ch;
        i += 1;
      } else {
        out += ch;
        i += 1;
      }
      continue;
    }

    if (state === LINE) {
      if (ch === '\n') {
        state = CODE;
        out += ch;
      } else {
        out += ' ';
      }
      i += 1;
      continue;
    }

    if (state === BLOCK) {
      if (ch === '*' && next === '/') {
        state = CODE;
        out += '  ';
        i += 2;
      } else {
        out += keepLayout(ch);
        i += 1;
      }
      continue;
    }

    // Inside a string or template literal.
    if (ch === '\\') {
      // Blank the escape and what it escapes, so \" never closes the run.
      out += next === undefined ? ' ' : '  ';
      i += 2;
      continue;
    }
    const closes =
      (state === SINGLE && ch === "'") ||
      (state === DOUBLE && ch === '"') ||
      (state === TEMPLATE && ch === '`');
    if (closes) {
      state = CODE;
      out += ch;
    } else {
      out += keepLayout(ch);
    }
    i += 1;
  }

  return out;
}

/** A file's staged content, or null when it cannot be read as text. */
function stagedContent(file) {
  try {
    return execFileSync('git', ['show', `:${file}`], { encoding: 'utf8' });
  } catch {
    return null;
  }
}

const message = (function formatMsg() {
  const COLORS = {
    green: '112',
    orange: '208',
    red: '160',
  };
  const mark = {
    ok: '\u2713',
    err: '\u2717',
  };

  const colorMsg = (msg, color) => `\u001b[38;5;${color}m${msg}\u001b[0m`;
  return {
    valid: (msg) => console.info(`  ${colorMsg(mark.ok, COLORS.green)} ${msg}`),
    error: (msg) => console.error(`  ${colorMsg(mark.err, COLORS.red)} ${msg}`),
    orange: (msg) => colorMsg(msg, COLORS.orange),
  };
})();

/**
 * This file necessarily contains every term it bans — `debugger` is one of the
 * TERMS keys — so it must never flag itself. Matching on basename rather than a
 * resolved path because it is checked from two different layouts: installed at
 * `.git/hooks/<name>`, and as source at `templates/hooks/<name>` in the
 * template repo, where the old path-relative comparison never matched and
 * editing this file was uncommittable.
 */
const selfName = path.basename(process.argv[1]);

function validStagedFilesPerTerm(status, [term, { git: gitRegex, js: jsRegex }]) {
  const gitCommand = `git diff --cached -G"${gitRegex}" --diff-filter=d --name-only`;
  const filesArray = execSync(gitCommand).toString().split(EOL);
  const matches = filesArray
    .map((file) => file.trim())
    .filter((file) => FILES_TO_SEARCH_IN.test(file))
    .filter((file) => path.basename(file) !== selfName)
    .filter((file) => {
      const content = stagedContent(file);
      // Unreadable (binary, or vanished between the two git calls): keep the
      // prefilter's verdict rather than silently clearing it.
      if (content === null) return true;
      return jsRegex.test(blankNonCode(content));
    });

  if (matches.length) {
    if (status === 0) {
      message.error('Unwanted terms found');
    }
    status = 1;
    console.info(`    The following files contains '${message.orange(term)}' in them:`);
    matches.map((line) => EOL + console.info('    - ' + message.orange(line)));
  }
  return status;
}

const status = Object.entries(TERMS).reduce(validStagedFilesPerTerm, 0);
if (status === 0) {
  message.valid('No unwanted terms where found');
}
process.exit(status);
