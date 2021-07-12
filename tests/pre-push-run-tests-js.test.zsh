#!/bin/zsh
TEST_NAME=`basename "$0"`
HOOK_CHECK=`echo ${0:a:h}/../templates/hooks/$TEST_NAME | sed 's@\.test@@'`
CURRENT_BRANCH=`git branch --show-current`

printf "Should detect js files and test them\n"
mkdir -p frontend/src
cat <<EOL > frontend/package.json
{
  "name": "test",
  "version": "1.0.0",
  "description": "",
  "main": "index.js",
  "scripts": {
    "test": "exit 0"
  },
  "author": "",
  "license": "ISC"
}
EOL
cd frontend
npm install semver &> /dev/null
cd -
git add frontend
git commit -m "Initial commit." &> /dev/null
COMMIT_SHA=`git rev-parse HEAD`
touch frontend/src/data1.js
git add frontend
git commit -m"add: data1" &> /dev/null
COMMIT_SHA2=`git rev-parse HEAD`
echo "origin/heads/main" $COMMIT_SHA2 "origin/heads/main" $COMMIT_SHA | $HOOK_CHECK &> /dev/null || exit 1

printf "Should detect js files and fail when test fails\n"
cat <<EOL > frontend/package.json
{
  "name": "test",
  "version": "1.0.0",
  "description": "",
  "main": "index.js",
  "scripts": {
    "test": "exit 1"
  },
  "author": "",
  "license": "ISC"
}
EOL
touch frontend/src/data2.js
git add frontend
git commit -m "another commit." &> /dev/null
COMMIT_SHA3=`git rev-parse HEAD`
echo "origin/heads/main" $COMMIT_SHA3 "origin/heads/main" $COMMIT_SHA2 | $HOOK_CHECK &> /dev/null && exit 1

exit 0
