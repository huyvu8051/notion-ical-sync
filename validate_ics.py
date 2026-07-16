import sys
from pathlib import Path

try:
    from icalendar import Calendar
except Exception as e:
    print(f"Missing dependency: {e}")
    sys.exit(1)

path = Path(sys.argv[1] if len(sys.argv) > 1 else "/tmp/calendar.ics")
data = path.read_bytes()
try:
    cal = Calendar.from_ical(data)
    print(f"OK components={len(list(cal.walk()))}")
    sys.exit(0)
except Exception as e:
    print(f"FAIL: {e}")
    sys.exit(1)
