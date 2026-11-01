#!/usr/bin/env python3
"""Open/update the fixture refresh PR on the configured Gitea origin."""
import json
import os
import urllib.request

# Called only by the trusted refresh workflow after its branch was pushed.
server = os.environ['GITHUB_SERVER_URL'].rstrip('/')
repo = os.environ['GITHUB_REPOSITORY']
token = os.environ['FIXTURE_PR_TOKEN']
branch = os.environ.get('FIXTURE_BRANCH', 'automation/installer-fixtures')
headers = {'Authorization': 'token ' + token, 'Content-Type': 'application/json'}
url = f'{server}/api/v1/repos/{repo}/pulls'
request = urllib.request.Request(url + '?state=open&limit=100', headers=headers)
with urllib.request.urlopen(request, timeout=30) as response:
    pulls = json.load(response)
if any(pr['head']['ref'] == branch for pr in pulls):
    print('Fixture refresh PR already exists; branch updated.')
else:
    body = {'title': 'Refresh pinned installer fixtures', 'head': branch, 'base': 'master',
            'body': 'Rebuilt using the committed committed Windows/Linux/macOS fixture builders. '
                    'Review binary inventories, source staleness/size checks and native test results.'}
    request = urllib.request.Request(url, data=json.dumps(body).encode(), headers=headers, method='POST')
    with urllib.request.urlopen(request, timeout=30) as response:
        print(json.load(response)['html_url'])
