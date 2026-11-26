"""Test helper: two friends on the local test server with a short conversation (replies, a reaction, a big emoji).
Prints the shared-prefs XML for the first account, to put on a test device."""
import json
import random
import sys
import urllib.request

BASE = "http://127.0.0.1:8790/api/account"


def call(method, path, body=None, token=None):
    request = urllib.request.Request(BASE + path, method=method, data=json.dumps(body).encode() if body is not None else None)
    request.add_header("content-type", "application/json")
    if token:
        request.add_header("authorization", "Bearer " + token)
    with urllib.request.urlopen(request) as r:
        return json.loads(r.read().decode())


n = random.randint(10000, 99999)
a = call("POST", "/register", {"email": f"cha{n}@example.test", "username": f"cha{n}", "password": "local-pass-1234567"})
b = call("POST", "/register", {"email": f"chb{n}@example.test", "username": f"chb{n}", "password": "local-pass-1234567"})
ta, tb, ia, ib = a["token"], b["token"], a["account"]["id"], b["account"]["id"]
call("POST", "/friends/request", {"username": f"chb{n}"}, ta)
call("POST", "/friends/accept", {"id": ia}, tb)
call("PATCH", "/me", {"display_name": "Minh Anh"}, tb)
m1 = call("POST", f"/messages/{ia}", {"body": "Tối nay nghe nhạc không?"}, tb)
call("POST", f"/messages/{ib}", {"body": "Ok, mấy giờ?", "reply_to": m1["id"]}, ta)
call("POST", f"/messages/{ia}", {"body": "8 giờ nhé, mình mở Roy Music"}, tb)
call("POST", f"/messages/{ia}", {"body": "👍"}, tb)
call("POST", f"/messages/{ib}/{m1['id']}/react", {"emoji": "❤️"}, ta)
profile = json.dumps({"id": ia, "display_name": "Hai", "username": f"cha{n}"}).replace('"', "&quot;")
xml = f"<?xml version='1.0' encoding='utf-8' standalone='yes' ?>\n<map>\n<string name=\"token\">{ta}</string>\n<string name=\"profile\">{profile}</string>\n</map>\n"
open(sys.argv[1], "w", encoding="utf-8").write(xml)
print("seeded", n)
