import base64
import json
import tempfile
import unittest
from pathlib import Path

import review

PNG = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Wl2n0kAAAAASUVORK5CYII="
)


class ReviewTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.sessions = Path(self.temp.name)
        self.image = self.sessions / "shot.png"
        self.image.write_bytes(PNG)

    def tearDown(self):
        self.temp.cleanup()

    def create(self):
        return review.create_session(
            self.sessions,
            self.image,
            "Button / default",
            "http://localhost:5016/button",
            "Check hover",
        )

    def test_create_and_list(self):
        meta = self.create()
        self.assertEqual("open", meta["status"])
        self.assertTrue((self.sessions / meta["id"] / "screenshot.png").is_file())
        self.assertEqual([meta["id"]], [item["id"] for item in review.list_sessions(self.sessions)])

    def test_submit_writes_the_agent_format(self):
        meta = self.create()
        annotations = [
            {"type": "rect", "start": {"x": 1, "y": 2}, "end": {"x": 30, "y": 40}, "color": "#e11d48", "width": 3},
            {"type": "text", "x": 5, "y": 6, "text": "Move this up", "color": "#f59e0b"},
        ]
        result = review.submit_annotations(self.sessions, meta["id"], annotations, None, 640, 360)

        folder = self.sessions / meta["id"]
        self.assertEqual("submitted", result["meta"]["status"])
        self.assertTrue((folder / "review.md").is_file())
        payload = json.loads((folder / "annotations.json").read_text())
        self.assertEqual(2, len(payload["annotations"]))
        self.assertEqual(640, payload["image"]["width"])
        self.assertIn("(5, 6): Move this up", (folder / "review.md").read_text())

    def test_invalid_session_id_is_rejected(self):
        with self.assertRaises(ValueError):
            review.session_dir(self.sessions, "../escape")


if __name__ == "__main__":
    unittest.main()
