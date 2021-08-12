#!/usr/bin/env node
// Run tests before push
// Author: https://github.com/fredericrous

const runTests = require('./common/run-tests');
const filesToDetect = /\.rb$/;

(async function () {
  try {
    const run = await runTests();
    run({
      rootFile: 'Rakefile',
      testCommand:
        'RAILS_ENV=test bundle exec bin/rake db:schema:load && bundle exec bin/rake db:migrate && bundle exec bin/rspec',
      filesToDetect,
    });
  } catch (e) {
    process.exit(1);
  }
})();
