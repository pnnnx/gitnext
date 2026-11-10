// Animals and an enclosure, to exercise C++ parsing.
#include <memory>
#include <string>
#include <vector>

namespace zoo {

/// Base class for every animal.
class Animal {
public:
    explicit Animal(std::string name) : name_(std::move(name)) {}
    virtual ~Animal() = default;
    /// Sound the animal makes.
    virtual std::string sound() const = 0;
    const std::string& name() const { return name_; }

protected:
    std::string name_;
};

class Dog : public Animal {
public:
    using Animal::Animal;
    std::string sound() const override { return "woof"; }
    void fetch(int times);
};

class Cat final : public Animal, private Comparable {
public:
    Cat(std::string name, int lives) : Animal(std::move(name)), lives_(lives) {}
    std::string sound() const override;
    bool operator<(const Cat& other) const { return lives_ < other.lives_; }

private:
    int lives_;
};

enum class Diet { Meat, Plants, Both };

struct Feeding {
    Diet diet;
    int grams;
};

template <typename T>
class Enclosure {
public:
    void add(std::unique_ptr<T> a) { animals_.push_back(std::move(a)); }
    std::size_t size() const { return animals_.size(); }
    std::vector<std::string> chorus() const;

private:
    std::vector<std::unique_ptr<T>> animals_;
};

void Dog::fetch(int times) {
    for (int i = 0; i < times; ++i) {
        sound();
    }
}

std::string Cat::sound() const { return lives_ > 1 ? "meow" : "..."; }

template <typename T>
std::vector<std::string> Enclosure<T>::chorus() const {
    std::vector<std::string> out;
    for (const auto& a : animals_) out.push_back(a->sound());
    return out;
}

/// Feeding plan for one animal.
Feeding plan_for(const Animal& a) {
    return Feeding{a.sound() == "woof" ? Diet::Meat : Diet::Both, 200};
}

}  // namespace zoo

int main() {
    zoo::Enclosure<zoo::Animal> pen;
    pen.add(std::make_unique<zoo::Dog>("rex"));
    pen.add(std::make_unique<zoo::Cat>("tom", 9));
    return static_cast<int>(pen.chorus().size());
}
