Feature: The inks a page puts down
  Overprint on anything but solid black text is advisory, white set to overprint vanishes on press, small text in a
  built black mis-registers (advisory), and the opt-in house rule keeps text on the K plate.

  Scenario: Solid black text under overprint is the normal case
    Given the fixture "black-text"
    When it is verified for print
    Then it passes

  Scenario: Cyan text under overprint
    Given the fixture "cyan-text"
    When it is verified for print
    Then check "overprint" fails on page 1
    And check "k-only-text" fails on page 1

  Scenario: White text set to overprint
    Given the fixture "white-overprint"
    When it is verified for print
    Then check "white-overprint" fails on page 1

  Scenario: Small text in a built black
    Given the fixture "rich-black-text"
    When it is verified for print
    Then check "rich-black" fails on page 1
