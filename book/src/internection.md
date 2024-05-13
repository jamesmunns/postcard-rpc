# Interacting with the board

We'll need to zoom out and look at three different crates in our `workbook` folder:

* `firmware` - our MCU firmware that we've been working on so far
* `workbook-host` - A crate for running on our PC
* `workbook-icd` - A crate for our protocol's type, `Endpoint`s, and `Topic`s

Let's look at the "ICD" crate first!
