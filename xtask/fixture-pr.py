#!/usr/bin/env python3
"""Open or update a fixture refresh PR on GitHub from a pushed branch.

No workflow runs this any more: fixture rebuilds are local only (see
crates/caskade-installer/docs/ci.md, Local-only runs). Run it by hand after
pushing FIXTURE_BRANCH (default automation/installer-fixtures), with a
FIXTURE_PR_TOKEN that has contents and pull requests write access and
GITHUB_REPOSITORY set to owner/name. A 403 prints the comparison link instead.
"""
import json
import os
import urllib.error
import urllib.request

api = os.environ.get('GITHUB_API_URL', 'https://api.github.com').rstrip('/')
server = os.environ.get('GITHUB_SERVER_URL', 'https://github.com').rstrip('/')
repo = os.environ['GITHUB_REPOSITORY']
token = os.environ['FIXTURE_PR_TOKEN']
branch = os.environ.get('FIXTURE_BRANCH', 'automation/installer-fixtures')
headers = {'Authorization': 'Bearer ' + token, 'Accept': 'application/vnd.github+json',
           'X-GitHub-Api-Version': '2022-11-28', 'Content-Type': 'application/json'}
url = f'{api}/repos/{repo}/pulls'
owner = repo.split('/')[0]
request = urllib.request.Request(f'{url}?state=open&head={owner}:{branch}', headers=headers)
with urllib.request.urlopen(request, timeout=30) as response:
    pulls = json.load(response)
if pulls:
    print(f'Fixture refresh PR already exists; branch updated: {pulls[0]["html_url"]}')
    raise SystemExit
body = {'title': 'Refresh pinned installer fixtures', 'head': branch, 'base': 'master',
        'body': 'Rebuilt using the committed Windows/Linux/macOS fixture builders. '
                'Review binary inventories, source staleness/size checks and native test results.'}
request = urllib.request.Request(url, data=json.dumps(body).encode(), headers=headers, method='POST')
try:
    with urllib.request.urlopen(request, timeout=30) as response:
        print(json.load(response)['html_url'])
except urllib.error.HTTPError as error:
    if error.code != 403:
        raise
    compare = f'{server}/{repo}/compare/master...{branch}?expand=1'
    message = f'GitHub refused to open the PR with this token ({error.code}). Open it here: {compare}'
    print('::warning::' + message)
    if summary := os.environ.get('GITHUB_STEP_SUMMARY'):
        with open(summary, 'a', encoding='utf-8') as stream:
            stream.write(message + '\n')
