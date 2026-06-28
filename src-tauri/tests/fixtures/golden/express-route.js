const express = require("express");
const router = express.Router();

function handler(req, res) {
    res.json({ ok: true });
}

router.post("/api/orders/:id", handler);

module.exports = router;
