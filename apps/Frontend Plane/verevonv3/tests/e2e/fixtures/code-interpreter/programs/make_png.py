# Reference program for the H5 "png" case. See programs/make_xlsx.py.
import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt

weeks = list(range(1, 9))
visits = [320, 355, 410, 390, 460, 505, 480, 540]
fig, ax = plt.subplots(figsize=(8, 4.5), dpi=100)
ax.plot(weeks, visits, marker="o", color="#2f6f9f")
ax.set_title("Besøkstall per uke")
ax.set_xlabel("Uke")
ax.set_ylabel("Antall besøk")
ax.grid(True, alpha=0.3)
fig.tight_layout()
fig.savefig("besokstall.png")
print("saved besokstall.png")
