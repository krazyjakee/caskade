#!/usr/bin/env python3
"""Write an e2e navigation script; does not start an app or send desktop input."""

import argparse

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--tours", type=int, default=10)
parser.add_argument("--settle-ms", type=int, default=300000)
parser.add_argument("--paint", choices=("cpu", "gpu"))
args = parser.parse_args()
if not 1 <= args.tours <= 50 or args.settle_ms < 0:
    parser.error("use 1–50 tours and a nonnegative settling time")


def open_link(href):
    print(f"focus-href {href}\npress confirm")


print("wait-text CASKADE")
open_link("/")
print("wait-text Welcome back")
if args.paint:
    print(f"wait-paint {args.paint}")
print(f"sleep {args.settle_ms}")
for tour in range(1, args.tours + 1):
    print(f"# Tour {tour}")
    open_link("/library")
    open_link("/library?filter=all")
    print("wait-text All Games")
    open_link("/collections")
    print("wait-text New Collection")
    open_link("/store")
    print("sleep 1000")
    open_link("/settings")
    # One full turn visits all 17 sections and returns to the first.
    for _ in range(17):
        print("press tab.next\nsleep 500")
    open_link("/")
    print(f"wait-text Welcome back\nsleep {10000 + tour}")
print("sleep 10000")
