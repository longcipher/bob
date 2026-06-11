Feature: Agent Loop Execution
  As a developer using the Bob framework
  I want the agent loop to execute turns correctly
  So that I can build reliable AI agent applications

  Background:
    Given a configured agent runtime with mock LLM and tools

  Scenario: Simple final response
    When the user sends "Hello"
    And the LLM responds with a final text "Hi there!"
    Then the agent should return the response "Hi there!"
    And the session should contain 2 messages

  Scenario: Tool call execution
    When the user sends "Read the file"
    And the LLM responds with a tool call to "read_file" with arguments {"path": "/tmp/test.txt"}
    And the tool "read_file" returns "file content"
    Then the agent should return a response containing "file content"
    And the session should contain 3 messages

  Scenario: Loop guard stops after max steps
    Given the turn policy has max_steps = 2
    When the user sends "Do something"
    And the LLM always responds with a tool call
    Then the agent should stop with guard reason "MaxSteps"
    And the response should indicate the turn was stopped

  Scenario: Consecutive error limit
    Given the turn policy has max_consecutive_errors = 2
    When the user sends "Try this"
    And the LLM responds with a tool call that fails
    Then the agent should stop after 2 consecutive errors
