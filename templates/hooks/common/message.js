#!/usr/bin/env node

const COLORS = {
  green: '112',
  orange: '208',
  red: '160',
};

module.exports = (function formatMsg() {
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
