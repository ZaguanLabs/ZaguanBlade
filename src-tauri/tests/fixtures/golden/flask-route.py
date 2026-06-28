from flask import Flask

app = Flask(__name__)


@app.route("/users/<int:id>", methods=["GET", "POST"])
def get_user(id):
    return {"id": id}
