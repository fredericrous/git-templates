#!/bin/zsh
TEST_NAME=`basename "$0"`
HOOK_CHECK=`echo ${0:a:h}/../templates/hooks/$TEST_NAME | sed 's@\.test.zsh@.js@'`
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

# The gate is typecheck → test:unit → test, cheapest first. Each script appends
# its name to a marker file so the test can assert BOTH that all three ran and
# that they ran in that order.
printf "Should run typecheck, test:unit and test, in that order\n"
cat <<EOL > frontend/package.json
{
  "name": "test",
  "version": "1.0.0",
  "scripts": {
    "typecheck": "echo typecheck >> ../ran.txt",
    "lint": "echo lint >> ../ran.txt",
    "test:unit": "echo test:unit >> ../ran.txt",
    "test": "echo test >> ../ran.txt"
  },
  "license": "ISC"
}
EOL
rm -f ran.txt
touch frontend/src/data3.js
git add frontend
git commit -m "add: data3" &> /dev/null
COMMIT_SHA4=`git rev-parse HEAD`
echo "origin/heads/main" $COMMIT_SHA4 "origin/heads/main" $COMMIT_SHA3 | $HOOK_CHECK &> /dev/null || exit 1
[ "`cat ran.txt`" = "typecheck
test:unit
test" ] || { printf "  got: %s\n" "`cat ran.txt | tr '\n' ' '`"; exit 1; }

# lint stays with pre-commit-lint-js (staged files, pinned eslint). Repeating it
# on push would cost time and catch nothing new.
printf "Should NOT run lint (pre-commit already lints)\n"
grep -q lint ran.txt && exit 1

printf "Should stop at the first failure, skipping the rest of the gate\n"
cat <<EOL > frontend/package.json
{
  "name": "test",
  "version": "1.0.0",
  "scripts": {
    "typecheck": "exit 1",
    "test:unit": "echo test:unit >> ../ran2.txt",
    "test": "echo test >> ../ran2.txt"
  },
  "license": "ISC"
}
EOL
rm -f ran2.txt
touch frontend/src/data4.js
git add frontend
git commit -m "add: data4" &> /dev/null
COMMIT_SHA5=`git rev-parse HEAD`
echo "origin/heads/main" $COMMIT_SHA5 "origin/heads/main" $COMMIT_SHA4 | $HOOK_CHECK &> /dev/null && exit 1
[ -f ran2.txt ] && exit 1  # typecheck failed → nothing after it should have run

# A package defining only some of the gate runs only those — the hook must not
# invent a missing script ("Missing script: test" would block the push).
printf "Should run only the gate scripts the package defines\n"
cat <<EOL > frontend/package.json
{
  "name": "test",
  "version": "1.0.0",
  "scripts": {
    "test:unit": "echo test:unit >> ../ran3.txt"
  },
  "license": "ISC"
}
EOL
rm -f ran3.txt
touch frontend/src/data5.js
git add frontend
git commit -m "add: data5" &> /dev/null
COMMIT_SHA6=`git rev-parse HEAD`
echo "origin/heads/main" $COMMIT_SHA6 "origin/heads/main" $COMMIT_SHA5 | $HOOK_CHECK &> /dev/null || exit 1
[ "`cat ran3.txt`" = "test:unit" ] || exit 1

exit 0
